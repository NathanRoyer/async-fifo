use core::task::{Waker, Context, Poll};
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::{Consumer, Storage};
use super::storage::TmpArray;

/// An error type returned when the channel is closed
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
    consumer: &'a Consumer<T>,
    storage: Option<S>,
    waker_box: Option<Box<Waker>>,
}

/// Future for Fifo consumption
///
/// Unlike [`Recv`], this future will use a mutably borrowed
/// storage object that you must provide.
pub struct Fill<'a, S: FillStorage<T>, T> {
    consumer: &'a Consumer<T>,
    storage: S,
    waker_box: Option<Box<Waker>>,
}

/// Future for Fifo consumption of one item
pub type RecvOne<'a, T> = Recv<'a, Option<T>, T>;

/// Future for Fifo consumption of `N` items
pub type RecvArray<'a, const N: usize, T> = Recv<'a, TmpArray<N, T>, T>;

/// Future for Fifo consumption of `N` items
pub type FillExact<'a, T> = Fill<'a, &'a mut [T], T>;

/// Future for Fifo consumption of `N` items
pub type FillMany<'a, T> = Fill<'a, &'a mut Vec<T>, T>;

/// Asynchronous equivalent of `try_recv_*` methods
///
/// # Example
///
/// ```rust
/// let (tx, [rx]) = async_fifo::new();
/// 
/// tx.send_iter(['a', 'b', 'c', 'd', 'e', 'f']);
/// core::mem::drop(tx);
/// 
/// let _task = async move {
///     while let Ok(array) = rx.recv_array::<3>().await {
///         println!("Pulled 3 items: {:?}", array);
///     }
/// 
///     // Err(Closed) is returned if all producers have been
///     // dropped AND all items have been consumed.
/// };
/// ```
impl<T: Unpin> Consumer<T> {
    /// Receives one item, asynchronously.
    pub fn recv(&self) -> RecvOne<'_, T> {
        self.into_recv()
    }

    /// Receives as many items as possible, into a vector, asynchronously.
    ///
    /// The number of received items is returned.
    pub fn recv_many<'a>(&'a self, vec: &'a mut Vec<T>) -> FillMany<'a, T> {
        self.into_fill(vec)
    }

    /// Receives exactly `slice.len()` items into a slice, asynchronously.
    pub fn recv_exact<'a>(&'a self, slice: &'a mut [T]) -> FillExact<'a, T> {
        self.into_fill(slice)
    }

    /// Receives exactly `N` items into an array, asynchronously.
    pub fn recv_array<const N: usize>(&self) -> RecvArray<'_, N, T> {
        self.into_recv()
    }

    /// Receives some items into custom storage, asynchronously.
    pub fn into_recv<S: RecvStorage<T>>(&self) -> Recv<'_, S, T> {
        Recv {
            consumer: self,
            waker_box: None,
            storage: None,
        }
    }

    /// Receives some items into custom storage, asynchronously.
    pub fn into_fill<S: FillStorage<T>>(&self, storage: S) -> Fill<'_, S, T> {
        Fill {
            consumer: self,
            waker_box: None,
            storage,
        }
    }
}

fn set_waker_check_closed<T>(
    waker_box: Option<Box<Waker>>,
    cx: &mut Context<'_>,
    consumer: &Consumer<T>,
) -> bool {
    if consumer.no_producers() {
        return true;
    }

    // get a waker box
    let waker = match waker_box {
        Some(mut waker) => {
            (*waker).clone_from(cx.waker());
            waker
        },
        None => Box::new(cx.waker().clone()),
    };

    // set it up
    consumer.insert_waker(waker);

    // maybe it was closed while we were inserting our waker
    consumer.no_producers()
}

impl<'a, S: RecvStorage<T>, T> Future for Recv<'a, S, T> {
    type Output = Result<S::Output, Closed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let storage = self.storage.take().unwrap_or_default();

        // first try
        let storage = match self.consumer.try_recv_into(storage) {
            Ok(result) => return Poll::Ready(Ok(result)),
            Err(storage) => storage,
        };

        let waker_box = self.waker_box.take();
        if set_waker_check_closed(waker_box, cx, &self.consumer) {
            return Poll::Ready(Err(Closed));
        }

        // second try
        match self.consumer.try_recv_into(storage) {
            Ok(result) => {
                // try to spare the waker box
                self.waker_box = self.consumer.take_waker();
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
        let len = self.consumer.fifo.try_recv(&mut self.storage);
        if len != 0 {
            return Poll::Ready(Ok(len));
        }

        let waker_box = self.waker_box.take();
        if set_waker_check_closed(waker_box, cx, &self.consumer) {
            return Poll::Ready(Err(Closed));
        }

        // second try
        let len = self.consumer.fifo.try_recv(&mut self.storage);
        if len != 0 {
            // try to spare the waker box
            self.waker_box = self.consumer.take_waker();
            Poll::Ready(Ok(len))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(any(feature = "blocking", doc))]
/// These methods are only available if you enable the `blocking` feature.
impl<T: Unpin> Consumer<T> {
    /// Receives one item, blocking.
    pub fn recv_blocking(&self) -> Result<T, Closed> {
        crate::blocking::block_on(self.recv())
    }

    /// Receives exactly `N` items into an array, blocking.
    pub fn recv_array_blocking<const N: usize>(&self) -> Result<[T; N], Closed> {
        crate::blocking::block_on(self.recv_array())
    }

    /// Receives as many items as possible, into a vector, blocking.
    pub fn recv_many_blocking(&self, vec: &mut Vec<T>) -> Result<usize, Closed> {
        crate::blocking::block_on(self.recv_many(vec))
    }

    /// Receives exactly `slice.len()` items into a slice, blocking.
    pub fn recv_exact_blocking(&self, slice: &mut [T]) -> Result<usize, Closed> {
        crate::blocking::block_on(self.recv_exact(slice))
    }
}

impl<T: Unpin, S: Storage<T> + Unpin> FillStorage<T> for S {}
impl<T: Unpin, S: FillStorage<T> + Default> RecvStorage<T> for S {}
