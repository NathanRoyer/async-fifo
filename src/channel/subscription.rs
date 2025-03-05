use core::task::Waker;
use core::iter::once;

use alloc::sync::Arc;

use crate::fifo::{FifoApi, SmallBlockSize};

#[derive(Clone)]
pub struct Subscribers {
    fifo: Arc<dyn FifoApi<Waker>>,
}

impl Default for Subscribers {
    fn default() -> Self {
        Self {
            fifo: SmallBlockSize::arc_fifo(),
        }
    }
}

impl Subscribers {
    pub fn notify_all(&self) {
        loop {
            let mut next_sub = None;
            self.fifo.pull(&mut next_sub);

            match next_sub {
                Some(sub) => sub.wake(),
                None => break,
            };
        }
    }

    pub fn subscribe(&self, waker: Waker) {
        self.fifo.push(&mut once(waker));
    }
}
