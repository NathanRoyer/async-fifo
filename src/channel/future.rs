use core::task::{Context, Poll};
use core::future::Future;
use core::pin::Pin;

use alloc::vec::Vec;

use super::async_wrapper::Receiver;
use crate::fifo::{TmpArray, Storage};

/// An error type returned when the channel is closed
///
/// A channel is considered closed if it has no items
/// and no senders.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Closed;

/// Asynchronous variant of [`Storage`] (Borrowing)
pub trait FillStorage<T>: Storage<T> + Unpin {}

/// Asynchronous variant of [`Storage`] (Defaulting)
pub trait RecvStorage<T>: FillStorage<T> + Default {}

/// Future for Fifo consumption
///
/// Unlike [`Fill`], this future will construct the backing
/// storage using its implementation of `Default`.
pub struct Recv<'a, S: RecvStorage<T>, T> {
    receiver: &'a Receiver<T>,
    storage: Option<S>,
}

/// Future for Fifo consumption
///
/// Unlike [`Recv`], this future will use a mutably borrowed
/// storage object that you must provide.
pub struct Fill<'a, S: FillStorage<T>, T> {
    receiver: &'a Receiver<T>,
    storage: S,
}

/// Future for Fifo consumption of one item
pub type RecvOne<'a, T> = Recv<'a, Option<T>, T>;

/// Future for Fifo consumption of `N` items
pub type RecvArray<'a, const N: usize, T> = Recv<'a, TmpArray<N, T>, T>;

/// Future for Fifo consumption of `N` items
pub type FillExact<'a, T> = Fill<'a, &'a mut [T], T>;

/// Future for Fifo consumption of `N` items
pub type FillMany<'a, T> = Fill<'a, &'a mut Vec<T>, T>;

impl<T: Unpin> Receiver<T> {
    /// Receives some items into custom storage, asynchronously.
    pub fn into_recv<S: RecvStorage<T>>(&self) -> Recv<'_, S, T> {
        Recv {
            receiver: self,
            storage: None,
        }
    }

    /// Receives some items into custom storage, asynchronously.
    pub fn into_fill<S: FillStorage<T>>(&self, storage: S) -> Fill<'_, S, T> {
        Fill {
            receiver: self,
            storage,
        }
    }
}

fn set_waker_check_no_prod<T>(
    cx: &mut Context<'_>,
    receiver: &Receiver<T>,
) -> bool {
    if receiver.no_producers() {
        return true;
    }

    // set it up
    receiver.subscribe(cx.waker().clone());

    // maybe it was closed while we were inserting our waker
    receiver.no_producers()
}

impl<'a, S: RecvStorage<T>, T> Future for Recv<'a, S, T> {
    type Output = Result<S::Output, Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let storage = self.storage.take().unwrap_or_default();

        // first try
        let storage = match self.receiver.try_recv_into(storage) {
            Ok(result) => {
                self.receiver.reset_flags();
                return Poll::Ready(Ok(result))
            },
            Err(storage) => storage,
        };

        if set_waker_check_no_prod(cx, &self.receiver) {
            self.receiver.reset_flags();
            return Poll::Ready(Err(Closed));
        }

        // second try
        match self.receiver.try_recv_into(storage) {
            Ok(result) => {
                self.receiver.reset_flags();
                Poll::Ready(Ok(result))
            },
            Err(storage) => {
                self.storage = Some(storage);
                Poll::Pending
            },
        }
    }
}

impl<'a, T: Unpin, S: FillStorage<T>> Future for Fill<'a, S, T> {
    type Output = Result<usize, Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // first try
        let len = self.receiver.fifo().pull(&mut self.storage);
        if len != 0 {
            return Poll::Ready(Ok(len));
        }

        if set_waker_check_no_prod(cx, &self.receiver) {
            return Poll::Ready(Err(Closed));
        }

        // second try
        match self.receiver.fifo().pull(&mut self.storage) {
            0 => Poll::Pending,
            len => Poll::Ready(Ok(len)),
        }
    }
}

impl<T: Unpin, S: Storage<T> + Unpin> FillStorage<T> for S {}
impl<T: Unpin, S: FillStorage<T> + Default> RecvStorage<T> for S {}
