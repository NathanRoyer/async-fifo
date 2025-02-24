//! ### Internal Details
//! 
//! Fifos internally own a chain of blocks.
//! 
//! - blocks have a configurable number of item slots (32 by default)
//! - they are allocated and appended to the chain by producers
//! - they are then populated by producers and consumed by consumers
//! - fully consumed first blocks get recycled and appended again at the end of the chain (when no one is visiting them anymore)
//! - the fifo has atomic cursors to track which block slots are available for production and consumption

use alloc::vec::Vec;

mod block_ptr;
mod block;
mod api;
mod tests;
mod storage;
mod async_api;

pub use api::{Producer, Consumer, BlockSize, new, new_vec, new_box};
pub use api::{SmallBlockSize, DefaultBlockSize, LargeBlockSize, HugeBlockSize};
pub use async_api::{Closed, Recv, RecvOne, RecvExact, AsyncStorage};

// Dyn-Compatible subset of the Storage trait, used by the `block` module.
//
// Automatically derived for every Storage implementor.
trait StorageCompat<T> {
    fn bounds(&self) -> (Option<usize>, Option<usize>);
    fn push(&mut self, index: usize, item: T);
    fn reserve(&mut self, len: usize);
}

/// Backing storage for receive operations
pub trait Storage<T>: Sized {
    type Output;

    fn push(&mut self, index: usize, item: T);
    fn finish(self, pushed: usize) -> Result<Self::Output, Self>;

    #[allow(unused_variables)]
    fn reserve(&mut self, len: usize) {}

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (None, None)
    }
}

impl<T, S: Storage<T>> StorageCompat<T> for S {
    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        S::bounds(self)
    }

    fn push(&mut self, index: usize, item: T) {
        S::push(self, index, item)
    }

    fn reserve(&mut self, len: usize) {
        S::reserve(self, len)
    }
}
