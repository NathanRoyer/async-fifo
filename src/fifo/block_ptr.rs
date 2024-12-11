// Almost all unsafe code is here.

use core::sync::atomic::{AtomicPtr, AtomicUsize};
use core::sync::atomic::Ordering::SeqCst;
use core::mem::{drop, forget};
use core::ptr::null_mut;

use alloc::boxed::Box;

use super::block::Block;
use crate::try_xchg_ptr;

#[repr(transparent)]
pub struct BlockPointer<const L: usize, const F: usize, T> {
    inner: AtomicPtr<Block<L, F, T>>,
}

impl<const L: usize, const F: usize, T> BlockPointer<L, F, T> {
    pub fn new() -> Self {
        Self {
            inner: AtomicPtr::new(null_mut()),
        }
    }

    /// Tries to load this block.
    pub fn load(&self) -> Option<&Block<L, F, T>> {
        let block_ptr = self.inner.load(SeqCst);
        // safety: this pointer can only be null or it
        // points to a memory location allocated with
        // Box::new() (see Self::append_new).
        // When the pointer is set to point to a just-
        // recycled block, this block could only have been
        // allocated with Self::append_new() previously.
        //
        // Multiple callers can access it concurrently
        // because all accesses are read-only.
        //
        // The block is freed via the drop implementation
        // defined at the bottom of this module.
        unsafe { block_ptr.as_ref() }
    }

    /// Tries to collect the chain's first block.
    ///
    /// Collection happens if the block has a next block
    /// and is fully consumed.
    ///
    /// Must only be called on the first block.
    pub fn try_collect(&self, revision: &AtomicUsize) -> Option<CollectedBlock<L, F, T>> {
        // only collect consumed blocks that have a next block
        let this = self.load()?;
        let next = this.next.load()?;

        // are we all done with this block?
        this.fully_consumed().then_some(())?;

        // the next's block offset
        assert_eq!(next.offset.load(SeqCst), 0);
        let offset = this.offset.load(SeqCst);
        next.offset.store(offset + L, SeqCst);

        let this_ptr = this as *const _ as *mut _;
        let next_ptr = next as *const _ as *mut _;

        // make the next block, the first block
        if try_xchg_ptr(&self.inner, this_ptr, next_ptr) {
            let collected = CollectedBlock {
                revision: revision.fetch_add(1, SeqCst),
                block_ptr: this_ptr,
            };

            Some(collected)
        } else {
            None
        }
    }

    fn append(&self, tail: *mut Block<L, F, T>) {
        let mut this = self;
        loop {
            match try_xchg_ptr(&this.inner, null_mut(), tail) {
                true => break,
                false => this = &this.load().unwrap().next,
            }
        }
    }

    /// Allocates a new block and adds it at the tail of the chain
    pub fn append_new(&self) {
        self.append(Box::leak(Box::default()));
    }

    /// Resets an old block and adds it at the tail of the chain.
    ///
    /// Make sure no Fifo visitor still has access to it.
    pub fn recycle(&self, collected: CollectedBlock<L, F, T>) {
        self.append(collected.recycle());
    }

    // See CollectedBlock::recycle()
    fn forget(&self) {
        self.inner.store(null_mut(), SeqCst);
    }
}

pub struct CollectedBlock<const L: usize, const F: usize, T> {
    pub revision: usize,
    block_ptr: *mut Block<L, F, T>,
}

impl<const L: usize, const F: usize, T> CollectedBlock<L, F, T> {
    fn recycle(self) -> *mut Block<L, F, T> {
        // prevent Drop from freeing this block
        let block_ptr = self.block_ptr;
        forget(self);

        // safety: see BlockPointer::load()
        let block = unsafe { &*block_ptr };

        // as part of this block being skipped,
        // the next block became the first block,
        // and so the Fifo owns it. We can safely
        // forget about it.
        block.next.forget();

        // reset the offset, which would be invalid.
        block.offset.store(0, SeqCst);

        // reset the produced and consumed flags.
        block.reset_flags();

        block_ptr
    }
}

impl<const L: usize, const F: usize, T> Drop for BlockPointer<L, F, T> {
    fn drop(&mut self) {
        let mut block_ptr = self.inner.swap(null_mut(), SeqCst);
        while !block_ptr.is_null() {
            // safety: see BlockPointer::load()
            // note: looping here to avoid recursion
            let block = unsafe { Box::from_raw(block_ptr) };
            block_ptr = self.inner.swap(null_mut(), SeqCst);
            drop(block);
        }
    }
}

impl<const L: usize, const F: usize, T> Drop for CollectedBlock<L, F, T> {
    fn drop(&mut self) {
        // safety: this pointer cannot be null because it comes from
        // BlockPointer::try_collect() which checks that it isn't.
        // It ultimately comes from BlockPointer::append_new().
        let block = unsafe { Box::from_raw(self.block_ptr) };
        // the next block isn't ours to free, as it's either still
        // part of the chain or in another collected block.
        block.next.forget();
        drop(block);
    }
}
