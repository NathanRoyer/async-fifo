//! This crate implements three lock-free structures for object transfers:
//! 
//! - fully-featured MPMC channels (in the [`fifo`] module)
//! - small one-shot SPSC channels (in the [`slot`] module)
//! - an `AtomicSlot<T>` type (in the [`slot`] module)
//! 
//! All of these structures are synchronized without any locks and without spinning/yielding.
//! This crate is compatible with `no_std` targets, except for the `*_blocking` methods.
//!
//! (All items of the `fifo` module are re-exported here.)

#![no_std]

#[cfg(any(test, feature = "blocking", doc))]
extern crate std;

extern crate alloc;

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering::{SeqCst, Relaxed};

/// Atomic and asynchronous slots
pub mod slot;

/// First-in, first-out MPMC channels (all items re-exported at crate root)
pub mod fifo;

pub use fifo::*;

#[cfg(any(feature = "blocking", doc))]
mod blocking;

fn try_swap_ptr<T>(atomic_ptr: &AtomicPtr<T>, old: *mut T, new: *mut T) -> bool {
    atomic_ptr.compare_exchange(old, new, SeqCst, Relaxed).is_ok()
}
