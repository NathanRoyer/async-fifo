use core::sync::atomic::{AtomicUsize, AtomicU8};
use core::sync::atomic::Ordering::{SeqCst, Relaxed};
use core::mem::MaybeUninit;
use core::cell::UnsafeCell;
use core::array::from_fn;
use core::task::Waker;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::slot::AtomicSlot;
use super::block_ptr::{BlockPointer, CollectedBlock};
use super::Storage;

type Slot<T> = UnsafeCell<MaybeUninit<T>>;
pub type RecycleBin<const L: usize, const F: usize, T> = Vec<CollectedBlock<L, F, T>>;

pub struct Block<const L: usize, const F: usize, T> {
    // updated by producers when they allocate new blocks
    // updated by anyone to recycle used blocks
    pub next: BlockPointer<L, F, T>,
    // only valid in the first block
    pub offset: AtomicUsize,
    // flags updated by producers as they write into their slots
    produced: [AtomicU8; F],
    // flags updated by consumers as they read produced slots
    consumed: [AtomicU8; F],
    slots: [Slot<T>; L],
}

impl<const L: usize, const F: usize, T> Block<L, F, T> {
    pub fn fully_consumed(&self) -> bool {
        (0..L).all(|i| get_slot_flag(&self.consumed, i))
    }

    pub fn reset_flags(&self) {
        for prod in &self.produced {
            prod.store(0, SeqCst);
        }

        for cons in &self.consumed {
            cons.store(0, SeqCst);
        }
    }
}

impl<const L: usize, const F: usize, T> Default for Block<L, F, T> {
    fn default() -> Self {
        Self {
            next: BlockPointer::new(),
            offset: AtomicUsize::new(0),
            produced: from_fn(|_i| AtomicU8::new(0)),
            consumed: from_fn(|_i| AtomicU8::new(0)),
            slots: from_fn(|_i| UnsafeCell::new(MaybeUninit::uninit())),
        }
    }
}

fn set_slot_flag<const F: usize>(slot_flags: &[AtomicU8; F], i: usize) {
    let mask = 1 << (i % 8);
    slot_flags[i / 8].fetch_or(mask, SeqCst);
}

fn get_slot_flag<const F: usize>(slot_flags: &[AtomicU8; F], i: usize) -> bool {
    let mask = 1 << (i % 8);
    (slot_flags[i / 8].load(SeqCst) & mask) > 0
}

fn try_xchg_int(atomic_int: &AtomicUsize, old: usize, new: usize) -> bool {
    atomic_int.compare_exchange(old, new, SeqCst, Relaxed).is_ok()
}

pub struct Fifo<const L: usize, const F: usize, T> {
    // updated by consumers when they collect fully consumed first blocks
    first_block: BlockPointer<L, F, T>,
    // updated by producers when they take slots
    prod_cursor: AtomicUsize,
    // updated by consumers when they take slots
    cons_cursor: AtomicUsize,
    // current revision
    revision: AtomicUsize,
    // Shared recycle bin for collected blocks
    recycle_bin: AtomicSlot<RecycleBin<L, F, T>>,
    // each visitor's current visit revision
    visitors: Box<[AtomicUsize]>,
    // the wakers of pending consumers
    wakers: Box<[AtomicSlot<Waker>]>,
}

impl<const L: usize, const F: usize, T> Fifo<L, F, T> {
    pub fn new(
        visitors: Box<[AtomicUsize]>,
        wakers: Box<[AtomicSlot<Waker>]>,
    ) -> Self {
        let recycle_bin = Box::new(RecycleBin::new());
        Self {
            first_block: BlockPointer::new(),
            prod_cursor: AtomicUsize::new(0),
            cons_cursor: AtomicUsize::new(0),
            revision: AtomicUsize::new(0),
            recycle_bin: AtomicSlot::new(recycle_bin),
            visitors,
            wakers,
        }
    }

    fn init_visit(&self, visitor_index: usize) {
        let rev = self.revision.load(SeqCst);
        self.visitors[visitor_index].store(rev, SeqCst);
    }

    fn stop_visit(&self, visitor_index: usize) {
        self.visitors[visitor_index].store(usize::MAX, SeqCst);
    }

    // visit must be ongoing
    fn produced(&self) -> usize {
        let mut is_first_block = true;
        let mut maybe_block = &self.first_block;
        let mut total_produced = 0;

        'outer: while let Some(block) = maybe_block.load() {
            if is_first_block {
                total_produced = block.offset.load(SeqCst);
                is_first_block = false;
            }

            for i in 0..L {
                match get_slot_flag(&block.produced, i) {
                    false => break 'outer,
                    true => total_produced += 1,
                }
            }

            maybe_block = &block.next;
        }

        total_produced
    }

    fn can_recycle(&self, collected: &CollectedBlock<L, F, T>) -> bool {
        for visitor in &self.visitors {
            if visitor.load(SeqCst) <= collected.revision {
                return false;
            }
        }

        true
    }

    fn try_maintain(&self) {
        if let Some(mut bin) = self.recycle_bin.try_take(false) {
            let gen = || self.first_block.try_collect(&self.revision);
            bin.extend(core::iter::from_fn(gen));

            while let Some(collected) = bin.first() {
                if self.can_recycle(collected) {
                    let collected = bin.remove(0);
                    self.first_block.recycle(collected);
                } else {
                    break;
                }
            }

            assert!(self.recycle_bin.try_insert(bin).is_ok());
        }
    }
}

pub trait FifoImpl<T> {
    fn send_iter(&self, iter: &mut dyn ExactSizeIterator<Item = T>, v: usize);
    fn try_recv(&self, storage: &mut dyn Storage<T>, v: usize) -> usize;
    fn insert_waker(&self, waker: Box<Waker>, v: usize);
    fn take_waker(&self, v: usize) -> Option<Box<Waker>>;
}

impl<const L: usize, const F: usize, T> FifoImpl<T> for Fifo<L, F, T> {
    fn send_iter(&self, iter: &mut dyn ExactSizeIterator<Item = T>, v: usize) {
        self.init_visit(v);

        let mut remaining = iter.len();
        let mut i = self.prod_cursor.fetch_add(remaining, SeqCst);

        let mut is_first_block = true;
        let mut block_offset = 0;
        let mut maybe_block = &self.first_block;

        while remaining > 0 {
            let Some(block) = maybe_block.load() else {
                maybe_block.append_new();
                continue;
            };

            if is_first_block {
                block_offset = block.offset.load(SeqCst);
                is_first_block = false;
            }

            let next_block_offset = block_offset + L;
            let block_range = block_offset..next_block_offset;

            // do we have slots here?
            while block_range.contains(&i) && remaining > 0 {
                let item = iter.next().unwrap();

                let slot_i = i - block_offset;
                let slot_cell_ptr = block.slots[slot_i].get();

                // safety: this pointer points to reachable memory
                // as it's part of the block, which is only freed
                // when all owners of this Arc<Fifo> have dropped
                // their handle.
                let slot_cell = unsafe { &mut *slot_cell_ptr };
                slot_cell.write(item);

                set_slot_flag(&block.produced, slot_i);

                i += 1;
                remaining -= 1;
            }

            block_offset = next_block_offset;
            maybe_block = &block.next;
        }

        self.stop_visit(v);
        self.try_maintain();

        for waker_slot in &self.wakers {
            if let Some(waker) = waker_slot.try_take(false) {
                waker.wake_by_ref();
            }
        }
    }

    fn try_recv(&self, storage: &mut dyn Storage<T>, v: usize) -> usize {
        let (min, max) = storage.bounds();
        let max = max.unwrap_or(usize::MAX);
        let min = min.unwrap_or(0);
        self.init_visit(v);

        let mut success = false;
        let mut i = 0;
        let mut negotiated = 0;

        while !success {
            let produced = self.produced();
            i = self.cons_cursor.load(SeqCst);
            negotiated = match produced.checked_sub(i) {
                Some(available) => available.min(max),
                None => continue,
            };

            if negotiated < min {
                negotiated = 0;
            }

            success = try_xchg_int(&self.cons_cursor, i, i + negotiated);
        }

        storage.reserve(negotiated);
        let mut remaining = negotiated;
        let mut is_first_block = true;
        let mut block_offset = 0;
        let mut maybe_block = &self.first_block;

        while remaining > 0 {
            let Some(block) = maybe_block.load() else {
                maybe_block.append_new();
                continue;
            };

            if is_first_block {
                block_offset = block.offset.load(SeqCst);
                is_first_block = false;
            }

            let next_block_offset = block_offset + L;
            let block_range = block_offset..next_block_offset;

            // do we have slots here?
            while block_range.contains(&i) && remaining > 0 {
                let slot_i = i - block_offset;
                assert!(get_slot_flag(&block.produced, slot_i));

                let slot_cell_ptr = block.slots[slot_i].get();

                // safety: this pointer points to reachable memory
                // as it's part of the block, which is only freed
                // when all owners of this Arc<Fifo> have dropped
                // their handle.
                // We also checked that this slot had the produced
                // flag set, which means its data is fully written.
                let item = unsafe {
                    let slot_cell = &mut *slot_cell_ptr;
                    slot_cell.assume_init_read()
                };

                let storage_index = negotiated - remaining;
                storage.push(storage_index, item);

                set_slot_flag(&block.consumed, slot_i);

                i += 1;
                remaining -= 1;
            }

            block_offset = next_block_offset;
            maybe_block = &block.next;
        }

        self.stop_visit(v);
        self.try_maintain();

        negotiated
    }

    fn insert_waker(&self, waker: Box<Waker>, v: usize) {
        self.wakers[v].insert(waker);
    }

    fn take_waker(&self, v: usize) -> Option<Box<Waker>> {
        self.wakers[v].try_take(false)
    }
}
