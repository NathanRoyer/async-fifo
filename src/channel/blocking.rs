use super::{Receiver, Closed};
use crate::block_on;
use alloc::vec::Vec;

/// These methods are only available if you enable the `blocking` feature.
impl<T: Unpin> Receiver<T> {
    /// Receives one item, blocking.
    pub fn recv_blocking(&mut self) -> Result<T, Closed> {
        block_on(self.recv())
    }

    /// Receives exactly `N` items into an array, blocking.
    pub fn recv_array_blocking<const N: usize>(&mut self) -> Result<[T; N], Closed> {
        block_on(self.recv_array())
    }

    /// Receives as many items as possible, into a vector, blocking.
    pub fn recv_many_blocking(&mut self, vec: &mut Vec<T>) -> Result<usize, Closed> {
        block_on(self.recv_many(vec))
    }

    /// Receives exactly `slice.len()` items into a slice, blocking.
    pub fn recv_exact_blocking(&mut self, slice: &mut [T]) -> Result<usize, Closed> {
        block_on(self.recv_exact(slice))
    }
}
