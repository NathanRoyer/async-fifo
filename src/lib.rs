#![doc = include_str!("../README.md")]
#![no_std]

#[cfg(test)]
extern crate std;

extern crate alloc;

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering::{SeqCst, Relaxed};

/// Atomic and asynchronous slots
pub mod slot;

/// First-in, first-out MPMC channels
pub mod fifo;

fn try_xchg_ptr<T>(atomic_ptr: &AtomicPtr<T>, old: *mut T, new: *mut T) -> bool {
    atomic_ptr.compare_exchange(old, new, SeqCst, Relaxed).is_ok()
}
