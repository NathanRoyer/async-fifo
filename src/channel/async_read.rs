use core::task::{Context, Poll};
use core::pin::Pin;

use futures_io::{Result as IoResult, Error, AsyncRead};
use futures_io::ErrorKind::BrokenPipe;

use crate::fifo::InternalStorageApi;

use super::{Receiver, set_waker_check_no_prod};

struct UpToLen<'a> {
    inner: &'a mut [u8],
}

impl InternalStorageApi<u8> for UpToLen<'_> {
    fn insert(&mut self, index: usize, item: u8) {
        self.inner[index] = item;
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(1), Some(self.inner.len()))
    }

    fn reserve(&mut self, _len: usize) {}
}

impl<'a> AsyncRead for Receiver<u8> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<IoResult<usize>> {
        let mut storage = UpToLen {
            inner: buf,
        };

        // first try
        let len = self.fifo().pull(&mut storage);
        if len != 0 {
            return Poll::Ready(Ok(len));
        }

        // subscribe
        if set_waker_check_no_prod(cx, &mut self) {
            return Poll::Ready(Err(Error::new(BrokenPipe, "Closed")));
        }

        // second try
        match self.fifo().pull(&mut storage) {
            0 => Poll::Pending,
            len => Poll::Ready(Ok(len)),
        }
    }
}
