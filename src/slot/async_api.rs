use core::task::{Waker, Context, Poll};
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::AtomicSlot;

struct AsyncSlot<T> {
    payload: AtomicSlot<T>,
    reader_waker: AtomicSlot<Waker>,
}

/// The writing side of an asynchronous slot
pub struct Writer<T> {
    inner: Arc<AsyncSlot<T>>,
}

/// The reading (polling) side of a asynchronous slot
pub struct Reader<T> {
    inner: Arc<AsyncSlot<T>>,
}

/// Create a new slot reader/writer pair
pub fn async_slot<T>() -> (Writer<T>, Reader<T>) {
    let inner = AsyncSlot {
        payload: AtomicSlot::NONE,
        reader_waker: AtomicSlot::NONE,
    };

    let inner = Arc::new(inner);

    let writer = Writer {
        inner: inner.clone(),
    };

    let reader = Reader {
        inner: inner.clone(),
    };

    (writer, reader)
}

impl<T> Writer<T> {
    /// Writes a value into this slot, waking up the reading side.
    pub fn write(self, item: Box<T>) {
        self.inner.payload.insert(item);
        if let Some(waker_box) = self.inner.reader_waker.try_take(false) {
            waker_box.wake_by_ref();
        }
    }
}

impl<T> Future for Reader<T> {
    type Output = Box<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(item) = self.inner.payload.try_take(false) {
            return Poll::Ready(item);
        }

        let waker = Box::new(cx.waker().clone());
        self.inner.reader_waker.insert(waker);

        match self.inner.payload.try_take(false) {
            Some(item) => Poll::Ready(item),
            None => Poll::Pending,
        }
    }
}
