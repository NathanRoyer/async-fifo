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
mod async_api;

pub use api::{Producer, Consumer, BlockSize, new, new_vec, new_box};
pub use api::{SmallBlockSize, DefaultBlockSize, LargeBlockSize, HugeBlockSize};
pub use async_api::{Closed, Recv, RecvOne, RecvExact, AsyncStorage};

trait StorageApi<T> {
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

impl<T, S: Storage<T>> StorageApi<T> for S {
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

impl<T> Storage<T> for Vec<T> {
    type Output = Self;

    fn reserve(&mut self, len: usize) {
        Vec::reserve(self, len);
    }

    fn push(&mut self, _index: usize, item: T) {
        self.push(item);
    }

    fn finish(self, pushed: usize) -> Result<Self::Output, Self> {
        match pushed {
            0 => Err(self),
            _ => Ok(self),
        }
    }
}

#[doc(hidden)]
pub struct ExactSizeVec<const N: usize, T>(Vec<T>);

impl<const N: usize, T> Default for ExactSizeVec<N, T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<const N: usize, T> Storage<T> for ExactSizeVec<N, T> {
    type Output = Vec<T>;

    fn reserve(&mut self, len: usize) {
        self.0.reserve(len);
    }

    fn push(&mut self, _index: usize, item: T) {
        self.0.push(item)
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(N), Some(N))
    }

    fn finish(self, pushed: usize) -> Result<Self::Output, Self> {
        match pushed {
            0 => Err(self),
            _ => Ok(self.0),
        }
    }
}

#[doc(hidden)]
pub struct TmpArray<const N: usize, T>([Option<T>; N]);

impl<const N: usize, T> Default for TmpArray<N, T> {
    fn default() -> Self {
        Self(core::array::from_fn(|_| None))
    }
}

impl<const N: usize, T> Storage<T> for TmpArray<N, T> {
    type Output = [T; N];

    fn push(&mut self, index: usize, item: T) {
        self.0[index] = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(N), Some(N))
    }

    fn finish(self, pushed: usize) -> Result<Self::Output, Self> {
        match pushed {
            0 => Err(self),
            _ => Ok(self.0.map(Option::unwrap)),
        }
    }
}

impl<T> Storage<T> for Option<T> {
    type Output = T;

    fn push(&mut self, _index: usize, item: T) {
        *self = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(1), Some(1))
    }

    fn finish(self, _pushed: usize) -> Result<Self::Output, Self> {
        self.ok_or(None)
    }
}
