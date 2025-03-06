use core::task::{Context, Poll};
use core::future::Future;
use core::pin::Pin;

use alloc::vec::Vec;

use crate::fifo::{TmpArray, Storage};
use super::async_api::Receiver;

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
///
/// This future is safely cancellable.
pub struct Recv<'a, S: RecvStorage<T>, T> {
    receiver: &'a mut Receiver<T>,
    storage: Option<S>,
}

/// Future for Fifo consumption
///
/// Unlike [`Recv`], this future will use a mutably borrowed
/// storage object that you must provide.
///
/// This future is safely cancellable.
pub struct Fill<'a, S: FillStorage<T>, T> {
    receiver: &'a mut Receiver<T>,
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
    pub fn into_recv<S: RecvStorage<T>>(&mut self) -> Recv<'_, S, T> {
        Recv {
            receiver: self,
            storage: None,
        }
    }

    /// Receives some items into custom storage, asynchronously.
    pub fn into_fill<S: FillStorage<T>>(&mut self, storage: S) -> Fill<'_, S, T> {
        Fill {
            receiver: self,
            storage,
        }
    }
}

pub(super) fn set_waker_check_no_prod<T: Unpin>(
    cx: &mut Context<'_>,
    receiver: &mut Receiver<T>,
) -> bool {
    if receiver.no_senders() {
        return true;
    }

    // set the waker up
    // prevent re-pushing the waker by watching a wake count
    let waker = cx.waker().clone();
    let last_wc = receiver.last_wake_count.take();
    let subs = receiver.subscribers();
    let current_wc = subs.subscribe(waker, last_wc);
    receiver.last_wake_count = Some(current_wc);

    // maybe the channel got closed while we were inserting our waker
    receiver.no_senders()
}

impl<'a, S: RecvStorage<T>, T: Unpin> Future for Recv<'a, S, T> {
    type Output = Result<S::Output, Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            receiver,
            storage,
        } = &mut *self;

        let taken_storage = storage.take().unwrap_or_default();

        // first try
        let taken_storage = match receiver.try_recv_into(taken_storage) {
            Ok(result) => return Poll::Ready(Ok(result)),
            Err(taken_storage) => taken_storage,
        };

        // subscribe
        let mut fail_ret = Poll::Pending;
        if set_waker_check_no_prod(cx, receiver) {
            fail_ret = Poll::Ready(Err(Closed));
        }

        // second try
        match receiver.try_recv_into(taken_storage) {
            Ok(result) => Poll::Ready(Ok(result)),
            Err(taken_storage) => {
                *storage = Some(taken_storage);
                fail_ret
            },
        }
    }
}

impl<'a, T: Unpin, S: FillStorage<T>> Future for Fill<'a, S, T> {
    type Output = Result<usize, Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            receiver,
            storage,
        } = &mut *self;

        // first try
        if let len @ 1.. = receiver.fifo().pull(storage) {
            return Poll::Ready(Ok(len));
        }

        // subscribe
        let mut fail_ret = Poll::Pending;
        if set_waker_check_no_prod(cx, receiver) {
            fail_ret = Poll::Ready(Err(Closed));
        }

        // second try
        match receiver.fifo().pull(storage) {
            0 => fail_ret,
            len => Poll::Ready(Ok(len)),
        }
    }
}

impl<T: Unpin, S: Storage<T> + Unpin> FillStorage<T> for S {}
impl<T: Unpin, S: FillStorage<T> + Default> RecvStorage<T> for S {}
