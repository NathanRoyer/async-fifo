//! Channels based on [`crate::fifo`]
//!
//! # Example
//!
//! ```rust
//! # use async_fifo::{block_on, Closed};
//! # block_on(async {
//! let (tx, rx) = async_fifo::new();
//! tx.send_iter(['a', 'b', 'c']);
//!
//! // Receive one by one
//! assert_eq!(rx.recv().await, Ok('a'));
//! assert_eq!(rx.recv().await, Ok('b'));
//! assert_eq!(rx.recv().await, Ok('c'));
//!
//! core::mem::drop(tx);
//! assert_eq!(rx.recv().await, Err(Closed));
//! # });
//! ```

use crate::fifo::DefaultBlockSize;

pub mod non_blocking;
mod async_wrapper;
mod future;

pub use async_wrapper::*;
pub use future::*;

#[doc(inline)]
pub use oneshot::new as oneshot;

#[cfg(any(feature = "blocking", doc))]
pub mod blocking;

pub mod oneshot;

/// Creates a channel with the default block size
pub fn new<T: 'static>() -> (Sender<T>, Receiver<T>) {
    DefaultBlockSize::channel()
}
