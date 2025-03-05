use std::task::{Context, Waker, Wake, Poll};
use std::thread::{Thread, park, current};
use std::future::Future;
use std::sync::Arc;
use std::pin::pin;

fn current_thread_waker() -> Waker {
    struct ThreadWaker {
        handle: Thread,
    }

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.handle.unpark();
        }
    }

    let thread_waker = ThreadWaker {
        handle: current(),
    };

    Arc::new(thread_waker).into()
}

pub fn block_on<T, F: Future<Output = T>>(fut: F) -> T {
    let waker = current_thread_waker();
    let mut context = Context::from_waker(&waker);
    let mut pinned = pin!(fut);

    loop {
        match pinned.as_mut().poll(&mut context) {
            Poll::Ready(retval) => break retval,
            Poll::Pending => park(),
        }
    }
}