use core::sync::atomic::AtomicUsize;
use core::array::from_fn;
use core::task::Waker;

use crate::slot::AtomicSlot;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;

use super::block::{Fifo, FifoImpl};
use super::{Storage, TmpArray};

/// Creates a Fifo with custom block size.
///
/// `L` is the block size.
/// `F` must be the block size divided by 8.
///
/// Panics if `F` isn't equal to `L / 8`;
pub fn with_block_size<
    const P: usize,
    const C: usize,
    const L: usize,
    const F: usize,
    T: 'static,
>() -> ([Producer<T>; P], [Consumer<T>; C]) {
    assert_eq!(F * 8, L);
    let mut recycle_bins = Vec::with_capacity(C + P);
    let mut visitors = Vec::with_capacity(C + P);
    let mut wakers = Vec::with_capacity(C);

    for _ in 0..C {
        recycle_bins.push(AtomicSlot::new(Box::new(vec![])));
        visitors.push(AtomicUsize::new(usize::MAX));
        wakers.push(AtomicSlot::default());
    }

    for _ in 0..P {
        recycle_bins.push(AtomicSlot::new(Box::new(vec![])));
        visitors.push(AtomicUsize::new(usize::MAX));
    }

    let fifo: Fifo<L, F, T> = Fifo::new(visitors.into(), wakers.into(), recycle_bins.into());

    let arc = Arc::new(fifo);

    let consumer = |i| Consumer {
        fifo: arc.clone(),
        visitor_index: i,
    };

    let producer = |i| Producer {
        fifo: arc.clone(),
        visitor_index: C + i,
    };

    (from_fn(producer), from_fn(consumer))
}

/// Creates a Fifo with reasonable default parameters.
pub fn new<const P: usize, const C: usize, T: 'static>() -> ([Producer<T>; P], [Consumer<T>; C]) {
    with_block_size::<P, C, 32, 4, T>()
}

/// Fifo Production Handle
pub struct Producer<T> {
    fifo: Arc<dyn FifoImpl<T>>,
    visitor_index: usize,
}

impl<T> Producer<T> {
    /// Sends a batch of items in the channel, atomically.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send_iter<I: ExactSizeIterator<Item = T>>(&mut self, mut iter: I) {
        self.fifo.send_iter(&mut iter, self.visitor_index);
    }

    /// Sends one item through the channel.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send(&mut self, item: T) {
        self.send_iter(core::iter::once(item));
    }
}

/// Fifo Consumtion Handle
pub struct Consumer<T> {
    fifo: Arc<dyn FifoImpl<T>>,
    visitor_index: usize,
}

unsafe impl<T> Send for Producer<T> {}
unsafe impl<T> Sync for Producer<T> {}

unsafe impl<T> Send for Consumer<T> {}
unsafe impl<T> Sync for Consumer<T> {}

impl<T> Consumer<T> {
    /// Tries to receive some items into custom storage.
    pub fn try_recv_into(&mut self, storage: &mut dyn Storage<T>) -> usize {
        self.fifo.try_recv(storage, self.visitor_index)
    }

    /// Tries to receive as many items as possible, into a vector.
    pub fn try_recv_many(&mut self) -> Vec<T> {
        let mut items = Vec::new();
        self.try_recv_into(&mut items);
        items
    }

    /// Tries to receive exactly `N` items into an array.
    pub fn try_recv_exact<const N: usize>(&mut self) -> Option<[T; N]> {
        let mut array = TmpArray(from_fn(|_| None));
        let len = self.try_recv_into(&mut array);
        (len == N).then(|| array.0.map(Option::unwrap))
    }

    /// Tries to receive one item.
    pub fn try_recv(&mut self) -> Option<T> {
        self.try_recv_exact().map(|[item]| item)
    }

    /// Sets the waker of the current task, to be woken up when new items are available.
    pub fn insert_waker(&self, waker: Box<Waker>) {
        self.fifo.insert_waker(waker, self.visitor_index);
    }

    /// Tries to take back a previously inserted waker.
    pub fn take_waker(&self) -> Option<Box<Waker>> {
        self.fifo.take_waker(self.visitor_index)
    }
}
