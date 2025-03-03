use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering::SeqCst;
use core::task::Waker;
use core::ops::Deref;
use core::iter::once;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::fifo::{FifoApi, BlockSize, SmallBlockSize};

use super::non_blocking::{Producer, Consumer};
use super::future::{RecvOne, RecvArray, FillMany, FillExact};

#[derive(Default)]
struct ReceiverFlags {
    cancelled: AtomicBool,
    woken_up: AtomicBool,
}

struct Subscription {
    waker: Waker,
    flags: Arc<ReceiverFlags>,
}

impl Subscription {
    /// Returns true if the subscription was cancelled
    pub fn notify(self) -> bool {
        self.waker.wake();
        self.flags.woken_up.store(true, SeqCst);
        self.flags.cancelled.load(SeqCst)
    }
}

#[derive(Clone)]
struct Subscribers {
    fifo: Arc<dyn FifoApi<Subscription>>,
}

impl Subscribers {
    fn notify_one(&self) {
        let mut keep_going = true;
        while keep_going {
            let mut next_sub = None;
            self.fifo.pull(&mut next_sub);
            keep_going = next_sub.map(Subscription::notify).unwrap_or(false);
        }
    }
}

/// Asynchronous FIFO sender
#[derive(Clone)]
pub struct Sender<T> {
    producer: Producer<T>,
    subscribers: Subscribers,
}

impl<T> Sender<T> {
    /// Sends a batch of items in the channel, atomically.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send_iter<I>(&self, into_iter: I)
    where
        I: IntoIterator,
        I::IntoIter: ExactSizeIterator<Item = T>,
    {
        self.producer.send_iter(into_iter);
        self.subscribers.notify_one();
    }

    /// Sends one item through the channel.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send(&self, item: T) {
        self.send_iter(core::iter::once(item));
    }
}

/// Asynchronous FIFO receiver
#[derive(Clone)]
pub struct Receiver<T> {
    consumer: Consumer<T>,
    subscribers: Subscribers,
    flags: Arc<ReceiverFlags>,
}

impl<T> Deref for Receiver<T> {
    type Target = Consumer<T>;

    fn deref(&self) -> &Consumer<T> {
        &self.consumer
    }
}

impl<T> Receiver<T> {
    pub(super) fn subscribe(&self, waker: Waker) {
        let subscription = Subscription {
            waker,
            flags: self.flags.clone(),
        };

        let mut iter = once(subscription);
        self.subscribers.fifo.push(&mut iter);
    }

    pub(super) fn reset_flags(&self) {
        self.flags.cancelled.store(false, SeqCst);
        self.flags.woken_up.store(false, SeqCst);
    }

    fn cancel(&self) {
        self.flags.cancelled.store(true, SeqCst);
        if self.flags.woken_up.load(SeqCst) {
            self.subscribers.notify_one();
        }
    }

    /// Returns true if all producers have been dropped
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, rx) = async_fifo::new();
    /// tx.send('z');
    ///
    /// // one remaining producer
    /// assert_eq!(rx.no_producers(), false);
    ///
    /// // drop it
    /// core::mem::drop(tx);
    ///
    /// // all producers are gone
    /// assert_eq!(rx.no_producers(), true);
    ///
    /// // No producer, yes, but one item is still in there.
    /// assert_eq!(rx.try_recv(), Some('z'));
    /// assert_eq!(rx.try_recv(), None);
    ///
    /// ```
    pub fn no_senders(&self) -> bool {
        self.consumer.no_producers()
    }
}

impl<T: Unpin> Receiver<T> {
    /// Receives one item, asynchronously.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, rx) = async_fifo::new();
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
    pub fn recv(&self) -> RecvOne<'_, T> {
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
    /// let (tx, rx) = async_fifo::new();
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
    pub fn recv_many<'a>(&'a self, vec: &'a mut Vec<T>) -> FillMany<'a, T> {
        self.into_fill(vec)
    }

    /// Receives exactly `slice.len()` items into a slice, asynchronously.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, rx) = async_fifo::new();
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
    pub fn recv_exact<'a>(&'a self, slice: &'a mut [T]) -> FillExact<'a, T> {
        self.into_fill(slice)
    }

    /// Receives exactly `N` items into an array, asynchronously.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_fifo::{block_on, Closed};
    /// # block_on(async {
    /// let (tx, rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    /// 
    /// // Pull a specific amount into an array
    /// assert_eq!(rx.recv_array().await, Ok(['a', 'b', 'c']));
    ///
    /// core::mem::drop(tx);
    /// assert_eq!(rx.recv_array::<3>().await, Err(Closed));
    /// # });
    /// ```
    pub fn recv_array<const N: usize>(&self) -> RecvArray<'_, N, T> {
        self.into_recv()
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl<const L: usize, const F: usize> BlockSize<L, F> {
    pub fn channel<T: 'static>() -> (Sender<T>, Receiver<T>) {
        let (producer, consumer) = Self::non_blocking();

        let subscribers = Subscribers {
            fifo: SmallBlockSize::arc_fifo(),
        };

        let sender = Sender {
            producer,
            subscribers: subscribers.clone(),
        };

        let receiver = Receiver {
            consumer,
            subscribers: subscribers.clone(),
            flags: Arc::new(ReceiverFlags::default()),
        };

        (sender, receiver)
    }
}
