use core::task::{Waker, Context, Poll};
use core::future::Future;
use core::array::from_fn;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::{Consumer, Storage, TmpArray};

/// Asynchronous extension to [`Storage`]
pub trait AsyncStorage<T>: Storage<T> + Unpin {
    type Output;
    fn finish(self, pushed: usize) -> Self::Output;
}

/// Future for Fifo consumption
pub struct Recv<'a, S: AsyncStorage<T>, T> {
    consumer: &'a Consumer<T>,
    storage: Option<S>,
    waker_box: Option<Box<Waker>>,
}

impl<T: Unpin> Consumer<T> {
    /// Receives some items into custom storage, asynchronously.
    pub fn recv_into<S: AsyncStorage<T>>(&self, storage: S) -> Recv<'_, S, T> {
        Recv {
            consumer: self,
            storage: Some(storage),
            waker_box: None,
        }
    }

    /// Receives as many items as possible, into a vector, asynchronously.
    pub fn recv_many(&self) -> Recv<'_, Vec<T>, T> {
        self.recv_into(Vec::new())
    }

    /// Receives exactly `N` items into an array, asynchronously.
    pub fn recv_exact<const N: usize>(&self) -> RecvExact<'_, N, T> {
        self.recv_into(TmpArray(from_fn(|_| None)))
    }

    /// Receives one item, asynchronously.
    pub fn recv(&self) -> RecvOne<'_, T> {
        self.recv_into(None)
    }
}

impl<'a, S: AsyncStorage<T>, T> Future for Recv<'a, S, T> {
    type Output = S::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut storage = self.storage.take().unwrap();

        // first try
        let num = self.consumer.try_recv_into(&mut storage);
        if num > 0 {
            return Poll::Ready(storage.finish(num));
        }

        // get a waker box
        let waker = match self.waker_box.take() {
            Some(mut waker) => {
                (*waker).clone_from(cx.waker());
                waker
            },
            None => Box::new(cx.waker().clone()),
        };

        // set it up
        self.consumer.insert_waker(waker);

        // second try
        let num = self.consumer.try_recv_into(&mut storage);
        if num > 0 {
            // try to spare a waker box
            self.waker_box = self.consumer.take_waker();
            Poll::Ready(storage.finish(num))
        } else {
            self.storage = Some(storage);
            Poll::Pending
        }
    }
}

#[cfg(any(feature = "blocking", doc))]
impl<T: Unpin> Consumer<T> {
    /// Receives some items into custom storage, blocking.
    ///
    /// This method is only available if you enable the `blocking` feature.
    pub fn recv_into_blocking<S: AsyncStorage<T>>(&self, storage: S) -> S::Output {
        crate::blocking::block_on(self.recv_into(storage))
    }

    /// Receives as many items as possible, into a vector, blocking.
    ///
    /// This method is only available if you enable the `blocking` feature.
    pub fn recv_many_blocking(&self) -> Vec<T> {
        crate::blocking::block_on(self.recv_many())
    }

    /// Receives exactly `N` items into an array, blocking.
    ///
    /// This method is only available if you enable the `blocking` feature.
    pub fn recv_exact_blocking<const N: usize>(&self) -> [T; N] {
        crate::blocking::block_on(self.recv_exact())
    }

    /// Receives one item, blocking.
    ///
    /// This method is only available if you enable the `blocking` feature.
    pub fn recv_blocking(&self) -> T {
        crate::blocking::block_on(self.recv())
    }
}

impl<T: Unpin> AsyncStorage<T> for Vec<T> {
    type Output = Self;
    fn finish(self, _pushed: usize) -> Self { self }
}

/// Future for Fifo consumption of one item
pub type RecvOne<'a, T> = Recv<'a, Option<T>, T>;

impl<T: Unpin> AsyncStorage<T> for Option<T> {
    type Output = T;
    fn finish(self, _pushed: usize) -> Self::Output {
        self.unwrap()
    }
}

/// Future for Fifo consumption of `N` items
pub type RecvExact<'a, const N: usize, T> = Recv<'a, TmpArray<N, T>, T>;

impl<const N: usize, T: Unpin> AsyncStorage<T> for TmpArray<N, T> {
    type Output = [T; N];
    fn finish(self, _pushed: usize) -> Self::Output {
        self.0.map(Option::unwrap)
    }
}
