use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::SeqCst;

use core::task::Waker;
use core::iter::once;

use alloc::sync::Arc;

use crate::fifo::{FifoApi, SmallBlockSize};

#[derive(Clone)]
pub struct Subscribers {
    fifo: Arc<dyn FifoApi<Waker>>,
    wake_count: Arc<AtomicUsize>,
}

impl Default for Subscribers {
    fn default() -> Self {
        Self {
            fifo: SmallBlockSize::arc_fifo(),
            wake_count: Arc::default(),
        }
    }
}

impl Subscribers {
    pub fn notify_all(&self) -> usize {
        self.wake_count.fetch_add(1, SeqCst);
        let wakers = self.fifo.iter();
        wakers.map(Waker::wake).count()
    }

    pub fn subscribe(&self, waker: Waker, last_wc: Option<usize>) -> usize {
        let current_wc = self.wake_count.load(SeqCst);

        if Some(current_wc) != last_wc {
            self.fifo.push(&mut once(waker));
        }

        current_wc
    }
}
