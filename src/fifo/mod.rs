//! First-in, first-out shared buffer

use core::sync::atomic::{AtomicUsize, AtomicU32};
use core::sync::atomic::Ordering::{SeqCst, Relaxed};
use core::array::from_fn;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::slot::Slot;

#[doc(hidden)]
pub use storage::TmpArray;

pub use storage::{Storage, InternalStorageApi};
pub use block_size::{BlockSize, SmallBlockSize, DefaultBlockSize, LargeBlockSize, HugeBlockSize};
use block_ptr::{BlockPointer, CollectedBlock};

mod block;
mod block_ptr;
mod block_size;
mod storage;

const REV_CAP_U32: u32 = 8;
const REV_CAP: usize = REV_CAP_U32 as usize;

type RecycleBin<const L: usize, const F: usize, T> = Vec<CollectedBlock<L, F, T>>;

fn try_swap_int(atomic_int: &AtomicUsize, old: usize, new: usize) -> bool {
    atomic_int.compare_exchange(old, new, SeqCst, Relaxed).is_ok()
}

/// Shared First-In/First-Out Buffer
///
/// # Principle
///
/// The methods on this object work by negociating how
/// many items can be pushed or pulled based on atomic
/// integer operations.
///
/// When you push N items, a reservation of N slots will be
/// made in the buffer. Once the reservation is made, the items
/// are written into the slots and they are marked as "produced".
///
/// When you try to pull some items from the buffer, first
/// a negociation takes place based on constraints specified
/// by the `Storage` implementation you provide. If the number
/// of available items meets these constraints, these items are
/// extracted from the buffer.
///
/// # Fifo vs Channel
///
/// Realistically, someone would use channels instead of this.
/// Channels provide distinct producer/consumer handles for one
/// FIFO, which makes code easier to understand.
///
/// # Memory Footprint
///
/// - This structure has a size of 68 bytes.
/// - It has a boxed 24-byte block recycle bin.
/// - The recycle bin has a growable buffer of 16-byte elements.
/// - Each block as a size of `16 + F x (2 + 8 x sz(item))` bytes.
///
/// For basic uses, the overall memory footprint of this structure
/// should be in the hundreds of bytes.
pub struct Fifo<const L: usize, const F: usize, T> {
    // updated by consumers when they collect fully consumed first blocks
    first_block: BlockPointer<L, F, T>,
    // updated by producers when they take slots
    prod_cursor: AtomicUsize,
    // updated by consumers when they take slots
    cons_cursor: AtomicUsize,
    // current revision
    revision: AtomicU32,
    // Shared recycle bin for collected blocks
    recycle_bin: Slot<RecycleBin<L, F, T>>,
    // ringbuf of visitors per revision
    visitors: [AtomicU32; REV_CAP],
}

impl<const L: usize, const F: usize, T> Default for Fifo<L, F, T> {
    fn default() -> Self {
        assert_eq!(F * 8, L);
        let recycle_bin = Box::new(RecycleBin::new());

        Self {
            first_block: BlockPointer::new(),
            prod_cursor: AtomicUsize::new(0),
            cons_cursor: AtomicUsize::new(0),
            revision: AtomicU32::new(0),
            recycle_bin: Slot::new(recycle_bin),
            visitors: from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl<const L: usize, const F: usize, T> Fifo<L, F, T> {
    fn init_visit(&self) -> usize {
        loop {
            let rev = self.revision.load(SeqCst) as usize;
            let rev_refcount = &self.visitors[rev % REV_CAP];
            rev_refcount.fetch_add(1, SeqCst);

            let new_rev = self.revision.load(SeqCst) as usize;
            match (new_rev - rev) < REV_CAP {
                true => break rev,
                // we have written into an already re-used refcount
                false => _ = rev_refcount.fetch_sub(1, SeqCst),
            }
        }
    }

    fn stop_visit(&self, rev: usize) {
        self.visitors[rev % REV_CAP].fetch_sub(1, SeqCst);
    }

    fn try_maintain(&self) {
        let Some(mut bin) = self.recycle_bin.try_take(false) else {
            // another thread is already trying to recycle the first block
            return;
        };

        let current_rev = self.revision.load(SeqCst) as usize;
        let oldest_rev = current_rev.saturating_sub(REV_CAP - 1);

        // find the oldest revision that still has at least one visitor
        let mut oldest_visited_rev = current_rev;
        for rev in oldest_rev..current_rev {
            let rc_slot = rev % REV_CAP;
            if self.visitors[rc_slot].load(SeqCst) != 0 {
                oldest_visited_rev = rev;
                break;
            }
        }

        let next_rev = current_rev + 1;
        let oldest_used_slot = oldest_visited_rev % REV_CAP;
        let next_slot = next_rev % REV_CAP;
        let can_increment = next_slot != oldest_used_slot;

        let mut i = 0;
        while i < bin.len() {
            if bin[i].revision < oldest_visited_rev {
                // quick, recycle these blocks, before we switch
                // to that refcount slot
                let block = bin.remove(i);
                self.first_block.recycle(block);
            } else {
                i += 1;
            }
        }

        if can_increment {
            let mut has_collected = false;

            while let Some(block) = self.first_block.try_collect(current_rev) {
                bin.push(block);
                has_collected = true;
            }

            if has_collected {
                self.revision.store(next_rev as u32, SeqCst);
            }
        }

        assert!(self.recycle_bin.try_insert(bin).is_ok());
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
                match block.is_produced(i) {
                    false => break 'outer,
                    true => total_produced += 1,
                }
            }

            maybe_block = &block.next;
        }

        total_produced
    }
}

unsafe impl<const L: usize, const F: usize, T> Send for Fifo<L, F, T> {}
unsafe impl<const L: usize, const F: usize, T> Sync for Fifo<L, F, T> {}

/// Dyn-Compatible subset of [`Fifo`] methods
pub trait FifoApi<T>: Send + Sync {
    fn push(&self, iter: &mut dyn ExactSizeIterator<Item = T>);
    fn pull(&self, storage: &mut dyn InternalStorageApi<T>) -> usize;
}

impl<const L: usize, const F: usize, T> FifoApi<T> for Fifo<L, F, T> {
    /// Inserts all the items from an iterator into the FIFO, atomically
    ///
    /// The order of the items is preserved, and they will be inserted
    /// consecutively; other pushes from other tasks will not interfere.
    ///
    /// This method doesn't spin, yield or sleeps; it should complete
    /// rather immediately. The only think that can take time here is
    /// an occasional memory allocation.
    fn push(&self, iter: &mut dyn ExactSizeIterator<Item = T>) {
        let revision = self.init_visit();

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
                block.produce(slot_i, item);

                i += 1;
                remaining -= 1;
            }

            block_offset = next_block_offset;
            maybe_block = &block.next;
        }

        self.stop_visit(revision);
        self.try_maintain();
    }

    /// Retrieves some elements from the FIFO, atomically
    ///
    /// If the number of available items doesn't meet the constraints
    /// of the `storage` parameter, this returns zero.
    ///
    /// This method doesn't spin, yield or sleeps; it should complete
    /// rather immediately.
    fn pull(&self, storage: &mut dyn InternalStorageApi<T>) -> usize {
        let (min, max) = storage.bounds();
        let max = max.unwrap_or(usize::MAX);
        let min = min.unwrap_or(0);
        let revision = self.init_visit();

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

            success = try_swap_int(&self.cons_cursor, i, i + negotiated);
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
                let item = block.consume(slot_i);

                let storage_index = negotiated - remaining;
                storage.insert(storage_index, item);

                i += 1;
                remaining -= 1;
            }

            block_offset = next_block_offset;
            maybe_block = &block.next;
        }

        self.stop_visit(revision);
        self.try_maintain();

        negotiated
    }
}
