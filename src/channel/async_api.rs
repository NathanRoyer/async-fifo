use core::ops::Deref;
use core::mem::drop;
use alloc::vec::Vec;

use crate::fifo::BlockSize;

use super::subscription::Subscribers;
use super::non_blocking::{Producer, Consumer};
use super::future::{RecvOne, RecvArray, FillMany, FillExact};

/// Asynchronous FIFO sender
pub struct Sender<T> {
    producer: Option<Producer<T>>,
    subscribers: Subscribers,
}

// I don't understand why deriving requires T: Clone
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            producer: self.producer.clone(),
            subscribers: self.subscribers.clone(),
        }
    }
}

impl<T> Sender<T> {
    /// Sends a batch of items in the channel, atomically.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    /// When the call returns, all items are ready to be received.
    pub fn send_iter<I>(&self, into_iter: I) -> usize
    where
        I: IntoIterator,
        I::IntoIter: ExactSizeIterator<Item = T>,
    {
        let producer = self.producer.as_ref();
        producer.unwrap().send_iter(into_iter);
        self.subscribers.notify_all()
    }

    /// Sends one item through the channel.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send(&self, item: T) -> usize {
        self.send_iter(core::iter::once(item))
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        drop(self.producer.take());
        self.subscribers.notify_all();
    }
}

/// Asynchronous FIFO receiver
#[derive(Clone)]
pub struct Receiver<T> {
    consumer: Consumer<T>,
    subscribers: Subscribers,
    pub(super) last_wake_count: Option<usize>,
}

impl<T> Deref for Receiver<T> {
    type Target = Consumer<T>;

    fn deref(&self) -> &Consumer<T> {
        &self.consumer
    }
}

impl<T> Receiver<T> {
    pub(super) fn subscribers(&self) -> &Subscribers {
        &self.subscribers
    }
}

impl<T: Unpin> Receiver<T> {
    /// Returns true if all senders have been dropped
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, mut rx) = async_fifo::new();
    /// tx.send('z');
    ///
    /// // one remaining sender
    /// assert_eq!(rx.no_senders(), false);
    ///
    /// // drop it
    /// core::mem::drop(tx);
    ///
    /// // all senders are gone
    /// assert_eq!(rx.no_senders(), true);
    ///
    /// // No sender, yes, but one item is still in there.
    /// assert_eq!(rx.try_recv(), Some('z'));
    /// assert_eq!(rx.try_recv(), None);
    ///
    /// ```
    pub fn no_senders(&self) -> bool {
        self.consumer.no_producers()
    }

    /// Receives one item, asynchronously.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, mut rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    ///
    /// // Receive one by one
    /// assert_eq!(rx.recv().await, Ok('a'));
    /// assert_eq!(rx.recv().await, Ok('b'));
    /// assert_eq!(rx.recv().await, Ok('c'));
    ///
    /// core::mem::drop(tx);
    /// assert_eq!(rx.recv().await, Err(Closed));
    /// # });
    /// ```
    pub fn recv(&mut self) -> RecvOne<'_, T> {
        self.into_recv()
    }

    /// Receives as many items as possible, into a vector, asynchronously.
    ///
    /// The number of received items is returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, mut rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c', 'd']);
    /// 
    /// // Pull as much as possible into a vec
    /// let mut bucket = Vec::new();
    /// assert_eq!(rx.recv_many(&mut bucket).await, Ok(4));
    /// assert_eq!(bucket, ['a', 'b', 'c', 'd']);
    ///
    /// core::mem::drop(tx);
    /// assert_eq!(rx.recv_many(&mut bucket).await, Err(Closed));
    /// # });
    /// ```
    pub fn recv_many<'a>(&'a mut self, vec: &'a mut Vec<T>) -> FillMany<'a, T> {
        self.into_fill(vec)
    }

    /// Receives exactly `slice.len()` items into a slice, asynchronously.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, mut rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    /// 
    /// // Pull a specific amount into a slice
    /// let mut buffer = ['_'; 3];
    /// assert_eq!(rx.recv_exact(&mut buffer).await, Ok(3));
    /// assert_eq!(buffer, ['a', 'b', 'c']);
    ///
    /// core::mem::drop(tx);
    /// assert_eq!(rx.recv_exact(&mut buffer).await, Err(Closed));
    /// # });
    /// ```
    pub fn recv_exact<'a>(&'a mut self, slice: &'a mut [T]) -> FillExact<'a, T> {
        self.into_fill(slice)
    }

    /// Receives exactly `N` items into an array, asynchronously.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, mut rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    /// 
    /// // Pull a specific amount into an array
    /// assert_eq!(rx.recv_array().await, Ok(['a', 'b', 'c']));
    ///
    /// core::mem::drop(tx);
    /// assert_eq!(rx.recv_array::<3>().await, Err(Closed));
    /// # });
    /// ```
    pub fn recv_array<const N: usize>(&mut self) -> RecvArray<'_, N, T> {
        self.into_recv()
    }
}

impl<const L: usize, const F: usize> BlockSize<L, F> {
    pub fn channel<T: 'static>() -> (Sender<T>, Receiver<T>) {
        let (producer, consumer) = Self::non_blocking();

        let subscribers = Subscribers::default();

        let sender = Sender {
            producer: Some(producer),
            subscribers: subscribers.clone(),
        };

        let receiver = Receiver {
            consumer,
            last_wake_count: None,
            subscribers: subscribers.clone(),
        };

        (sender, receiver)
    }
}
