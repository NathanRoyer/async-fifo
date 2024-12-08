use core::task::{Waker, Context, Poll};
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::AtomicSlot;

struct OneShot<T> {
    payload: AtomicSlot<T>,
    consumer_waker: AtomicSlot<Waker>,
}

/// The writing side of the oneshot channel
pub struct Producer<T> {
    inner: Arc<OneShot<T>>,
}

/// The reading side of the oneshot channel
pub struct Consumer<T> {
    inner: Arc<OneShot<T>>,
}

/// Create a new oneshot channel, returning a producer/consumer pair
pub fn oneshot<T>() -> (Producer<T>, Consumer<T>) {
    let inner = OneShot {
        payload: AtomicSlot::NONE,
        consumer_waker: AtomicSlot::NONE,
    };

    let inner = Arc::new(inner);

    let producer = Producer {
        inner: inner.clone(),
    };

    let consumer = Consumer {
        inner: inner.clone(),
    };

    (producer, consumer)
}

impl<T> Producer<T> {
    /// Sends a value into this channel, waking up the receiving side.
    pub fn send(self, item: Box<T>) {
        self.inner.payload.insert(item);
        if let Some(waker_box) = self.inner.consumer_waker.try_take(false) {
            waker_box.wake_by_ref();
        }
    }
}

#[cfg(any(feature = "blocking", doc))]
impl<T> Consumer<T> {
    /// Waits for a value to be received from the channel.
    ///
    /// Do not use this in asynchronous code, instead directly use the consumer
    /// as a future, which will yield the item once it has been received.
    ///
    /// This method is only available if you enable the `blocking` feature.
    pub fn recv_blocking(self) -> T {
        *crate::blocking::block_on(self)
    }
}

impl<T> Future for Consumer<T> {
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
