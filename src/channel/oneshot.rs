//! # One shot SPSC channels
//! 
//! Very simple single-use channels with a sync and async API, blocking/non-blocking.
//! This builds on [`Slot<T>`], so the item has to be boxed.
//! Most useful for the transfer of return values in the context of remote procedure calling, as "reply pipes".
//! 
//! ```rust
//! # async_fifo::block_on(async {
//! let (tx, rx) = async_fifo::oneshot();
//! 
//! let item = 72u8;
//! tx.send(Box::new(item));
//! 
//! assert_eq!(*rx.await, item);
//! # });
//! ```

use core::task::{Waker, Context, Poll};
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::slot::Slot;

struct OneShot<T> {
    payload: Slot<T>,
    consumer_waker: Slot<Waker>,
}

/// The writing side of the oneshot channel
pub struct Sender<T> {
    inner: Arc<OneShot<T>>,
}

/// The reading side of the oneshot channel
pub struct Receiver<T> {
    inner: Arc<OneShot<T>>,
}

/// Create a new oneshot channel, returning a sender/receiver pair
pub fn new<T>() -> (Sender<T>, Receiver<T>) {
    let inner = OneShot {
        payload: Slot::NONE,
        consumer_waker: Slot::NONE,
    };

    let inner = Arc::new(inner);

    let sender = Sender {
        inner: inner.clone(),
    };

    let receiver = Receiver {
        inner: inner.clone(),
    };

    (sender, receiver)
}

impl<T> Sender<T> {
    /// Sends a value into this channel, waking up the receiving side.
    pub fn send(self, item: Box<T>) {
        self.inner.payload.insert(item);
        if let Some(waker_box) = self.inner.consumer_waker.try_take(false) {
            waker_box.wake_by_ref();
        }
    }
}

#[cfg(any(feature = "blocking", doc))]
impl<T> Receiver<T> {
    /// Waits for a value to be received from the channel.
    ///
    /// Do not use this in asynchronous code, instead directly use the receiver
    /// as a future, which will yield the item once it has been received.
    ///
    /// This method is only available if you enable the `blocking` feature.
    pub fn recv_blocking(self) -> T {
        *crate::block_on(self)
    }
}

impl<T> Future for Receiver<T> {
    type Output = Box<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(item) = self.inner.payload.try_take(false) {
            return Poll::Ready(item);
        }

        let waker = Box::new(cx.waker().clone());
        self.inner.consumer_waker.insert(waker);

        match self.inner.payload.try_take(false) {
            Some(item) => Poll::Ready(item),
            None => Poll::Pending,
        }
    }
}
