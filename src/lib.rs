#![doc = include_str!("../README.md")]
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

#[doc(hidden)]
pub use fifo::*;

#[cfg(any(feature = "blocking", doc))]
mod blocking;

fn try_xchg_ptr<T>(atomic_ptr: &AtomicPtr<T>, old: *mut T, new: *mut T) -> bool {
    atomic_ptr.compare_exchange(old, new, SeqCst, Relaxed).is_ok()
}
