use core::sync::atomic::{AtomicUsize, AtomicU8};
use core::sync::atomic::Ordering::SeqCst;
use core::mem::MaybeUninit;
use core::cell::UnsafeCell;
use core::array::from_fn;

use super::block_ptr::BlockPointer;

type Slot<T> = UnsafeCell<MaybeUninit<T>>;

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

    pub fn is_produced(&self, i: usize) -> bool {
        get_slot_flag(&self.produced, i)
    }

    pub fn produce(&self, i: usize, item: T) {
        assert!(!get_slot_flag(&self.produced, i));
        let slot_cell_ptr = self.slots[i].get();

        // safety: this pointer points to reachable memory
        // as it's part of the block, which is only freed
        // when all owners of this Arc<Fifo> have dropped
        // their handle.
        let slot_cell = unsafe { &mut *slot_cell_ptr };
        slot_cell.write(item);

        set_slot_flag(&self.produced, i);
    }

    pub fn consume(&self, i: usize) -> T {
        assert!(get_slot_flag(&self.produced, i));
        assert!(!get_slot_flag(&self.consumed, i));
        let slot_cell_ptr = self.slots[i].get();

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

        set_slot_flag(&self.consumed, i);

        item
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
