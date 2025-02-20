use core::array::from_fn;
use core::task::Waker;

use crate::slot::AtomicSlot;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::block::{Fifo, FifoImpl};
use super::{Storage, TmpArray};

#[derive(Default)]
/// Custom Block Size
///
/// In channels, items are stored in contiguous blocks (chunks) of item slots.
/// New blocks are allocated as items are send through the channel.
/// A large block size will result in less but bigger allocations.
/// A small block size will result in more, smaller allocations.
/// For channels transporting large amounts of items, a large block size is preferred.
/// This structure allows you to create channels with custom block size (chunk length).
///
/// See the following pre-defined block sizes:
/// - [`SmallBlockSize`]
/// - [`DefaultBlockSize`]
/// - [`LargeBlockSize`]
/// - [`HugeBlockSize`]
///
/// L must be equal to F x 8
pub struct BlockSize<const L: usize, const F: usize>;

impl<const L: usize, const F: usize> BlockSize<L, F> {
    fn build_n<T: 'static>(num_rx: usize) -> (Producer<T>, Arc<dyn FifoImpl<T>>) {
        assert_eq!(F * 8, L);
        let wakers = (0..num_rx).map(|_| AtomicSlot::default()).collect();
        let fifo: Fifo<L, F, T> = Fifo::new(Vec::into(wakers));

        let arc = Arc::new(fifo);

        let producer = Producer {
            fifo: arc.clone(),
        };

        (producer, arc)
    }

    /// Creates a Fifo with this block size.
    ///
    /// `N` is the number of consumers.
    ///
    /// Panics if `F` isn't equal to `L / 8`;
    pub fn build<const N: usize, T: 'static>() -> (Producer<T>, [Consumer<T>; N]) {
        let (tx, fifo) = Self::build_n::<T>(N);

        let rx_array = from_fn(|i| Consumer {
            fifo: fifo.clone(),
            waker_index: i,
        });

        (tx, rx_array)
    }

    pub fn build_vec<T: 'static>(num_rx: usize) -> (Producer<T>, Vec<Consumer<T>>) {
        let (tx, fifo) = Self::build_n::<T>(num_rx);
        let mut rx_vec = Vec::with_capacity(num_rx);

        for i in 0..num_rx {
            rx_vec.push(Consumer {
                fifo: fifo.clone(),
                waker_index: i,
            })
        }

        (tx, rx_vec)
    }

    pub fn build_box<const N: usize, T: 'static>() -> (Producer<T>, Box<[Consumer<T>; N]>) {
        let (tx, rx_vec) = Self::build_vec::<T>(N);
        match Box::try_from(rx_vec) {
            Ok(rx_box) => (tx, rx_box),
            Err(_) => unreachable!()
        }
    }
}

/// Block size suitable for sending items one by one, from time to time
///
/// Each block will have 8 slots.
pub type SmallBlockSize = BlockSize<8, 1>;

/// Reasonable default block size
///
/// Each block will have 32 slots.
pub type DefaultBlockSize = BlockSize<32, 4>;

/// Block size suitable for batch sending of many items
///
/// Each block will have 4096 slots.
pub type LargeBlockSize = BlockSize<4096, 512>;

/// Block size suitable for batch sending of tons of items
///
/// Each block will have 1 048 576 slots.
pub type HugeBlockSize = BlockSize<1048576, 131072>;

/// Creates a Fifo with default block size (Array of Consumers)
pub fn new<const N: usize, T: 'static>() -> (Producer<T>, [Consumer<T>; N]) {
    DefaultBlockSize::build()
}

/// Creates a Fifo with default block size (Vec of Consumers)
pub fn new_vec<T: 'static>(num_consumers: usize) -> (Producer<T>, Vec<Consumer<T>>) {
    DefaultBlockSize::build_vec::<T>(num_consumers)
}

/// Creates a Fifo with default block size (Boxed Array of Consumers)
pub fn new_box<const N: usize, T: 'static>() -> (Producer<T>, Box<[Consumer<T>; N]>) {
    DefaultBlockSize::build_box::<N, T>()
}

/// Fifo Production Handle (implements `Clone`)
pub struct Producer<T> {
    fifo: Arc<dyn FifoImpl<T>>,
}

impl<T> Clone for Producer<T> {
    fn clone(&self) -> Self {
        let fifo = self.fifo.clone();
        fifo.inc_num_prod();
        Self {
            fifo,
        }
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        self.fifo.dec_num_prod();
    }
}

impl<T> Producer<T> {
    /// Sends a batch of items in the channel, atomically.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send_iter<I: ExactSizeIterator<Item = T>>(&self, mut iter: I) {
        self.fifo.send_iter(&mut iter);
    }

    /// Sends one item through the channel.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send(&self, item: T) {
        self.send_iter(core::iter::once(item));
    }
}

/// Fifo Consumption Handle
pub struct Consumer<T> {
    fifo: Arc<dyn FifoImpl<T>>,
    waker_index: usize,
}

unsafe impl<T> Send for Producer<T> {}
unsafe impl<T> Sync for Producer<T> {}

unsafe impl<T> Send for Consumer<T> {}
unsafe impl<T> Sync for Consumer<T> {}

impl<T> Consumer<T> {
    /// Returns true if all producers have been dropped
    pub fn is_closed(&self) -> bool {
        self.fifo.is_closed()
    }

    /// Tries to receive some items into custom storage.
    pub fn try_recv_into(&self, storage: &mut dyn Storage<T>) -> usize {
        self.fifo.try_recv(storage)
    }

    /// Tries to receive as many items as possible, into a vector.
    pub fn try_recv_many(&self) -> Vec<T> {
        let mut items = Vec::new();
        self.try_recv_into(&mut items);
        items
    }

    /// Tries to receive exactly `N` items into an array.
    pub fn try_recv_exact<const N: usize>(&self) -> Option<[T; N]> {
        let mut array = TmpArray(from_fn(|_| None));
        let len = self.try_recv_into(&mut array);
        (len == N).then(|| array.0.map(Option::unwrap))
    }

    /// Tries to receive one item.
    pub fn try_recv(&self) -> Option<T> {
        self.try_recv_exact().map(|[item]| item)
    }

    /// Sets the waker of the current task, to be woken up when new items are available.
    pub fn insert_waker(&self, waker: Box<Waker>) {
        self.fifo.insert_waker(waker, self.waker_index);
    }

    /// Tries to take back a previously inserted waker.
    pub fn take_waker(&self) -> Option<Box<Waker>> {
        self.fifo.take_waker(self.waker_index)
    }
}
