mod block_ptr;
mod block;
mod api;
mod tests;
mod async_api;

pub use api::{Producer, Consumer, BlockSize, new, new_vec, new_box};
pub use api::{SmallBlockSize, DefaultBlockSize, LargeBlockSize, HugeBlockSize};
pub use async_api::{Closed, Recv, RecvOne, RecvExact, AsyncStorage};

/// Backing storage for receive operations
pub trait Storage<T> {
    fn push(&mut self, index: usize, item: T);
    #[allow(unused_variables)]
    fn reserve(&mut self, len: usize) {}
    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (None, None)
    }
}

impl<T> Storage<T> for alloc::vec::Vec<T> {
    fn reserve(&mut self, len: usize) {
        self.reserve(len);
    }

    fn push(&mut self, _index: usize, item: T) {
        self.push(item);
    }
}

#[doc(hidden)]
pub struct TmpArray<const N: usize, T>([Option<T>; N]);

impl<const N: usize, T> Storage<T> for TmpArray<N, T> {
    fn push(&mut self, index: usize, item: T) {
        self.0[index] = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(N), Some(N))
    }
}

impl<T> Storage<T> for Option<T> {
    fn push(&mut self, _index: usize, item: T) {
        *self = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(1), Some(1))
    }
}
