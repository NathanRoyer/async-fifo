//! This crate implements multiple lock-free structures for object transfers:
//! 
//! - fully-featured MPMC channels (see [`channel::new()`])
//! - small one-shot SPSC channels (see [`channel::oneshot()`])
//! - an atomic equivalent to `Box<T>` (see [`slot::Slot`])
//! 
//! All of these structures are synchronized without any locks and without spinning/yielding.
//! This crate is compatible with `no_std` targets, except for the `*_blocking` methods.
//!
//! (All items of the `channel` module are re-exported here at the root.)

#![no_std]

#[cfg(any(test, doc, feature = "blocking"))]
extern crate std;

extern crate alloc;

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering::{SeqCst, Relaxed};

mod tests;

/// Atomic and asynchronous slots
pub mod slot;

pub mod fifo;

pub mod channel;

pub use channel::*;

#[cfg(any(test, doc, feature = "blocking"))]
mod fn_block_on;

#[cfg(any(test, doc, feature = "blocking"))]
#[doc(hidden)]
pub use fn_block_on::block_on;

fn try_swap_ptr<T>(atomic_ptr: &AtomicPtr<T>, old: *mut T, new: *mut T) -> bool {
    atomic_ptr.compare_exchange(old, new, SeqCst, Relaxed).is_ok()
}
