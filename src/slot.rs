//! # `Slot<T>`
//! 
//! You can atomically swap the contents of this slot from any thread.
//! Think of it as `Mutex<Option<Box<T>>>` without any actual locking.

use core::sync::atomic::Ordering::SeqCst;
use core::sync::atomic::AtomicPtr;

use alloc::boxed::Box;

use crate::try_swap_ptr;

/// Atomically Swappable `Option<Box<T>>`
pub struct Slot<T> {
    inner: AtomicPtr<T>,
}

impl<T> Slot<T> {
    const EMPTY: *mut T = 1 as *mut T;
    const LOCKED: *mut T = 2 as *mut T;

    pub const NONE: Self = Self {
        inner: AtomicPtr::new(Self::EMPTY)
    };

    /// Creates a new `Slot` with an initial item inside.
    ///
    /// To create an empty slot, use `Slot::default()` or `Slot::NONE`.
    pub fn new(item: Box<T>) -> Self {
        let slot = Self::default();
        assert!(slot.try_insert(item).is_ok());
        slot
    }

    /// Tries to push an item into this slot, failing if it's occupied or locked.
    pub fn try_insert(&self, item: Box<T>) -> Result<(), Box<T>> {
        let item_ptr = Box::leak(item);
        match try_swap_ptr(&self.inner, Self::EMPTY, item_ptr) {
            true => Ok(()),
            false => Err(unsafe { Box::from_raw(item_ptr) }),
        }
    }

    /// Forcibly pushes an item into this slot, returning the previous content
    ///
    /// This discards the "locked" flag.
    pub fn insert(&self, item: Box<T>) -> Option<Box<T>> {
        let item_ptr = Box::leak(item);
        match self.inner.swap(item_ptr, SeqCst) {
            p if p == (Self::EMPTY) || (p == Self::LOCKED) => None,
            other => Some(unsafe { Box::from_raw(other) }),
        }
    }

    /// Tries to extract the contained item.
    ///
    /// Setting `lock` to `true` will prevent pushing into this slot again.
    pub fn try_take(&self, lock: bool) -> Option<Box<T>> {
        let item_ptr = match self.inner.load(SeqCst) {
            p if p == (Self::EMPTY) || (p == Self::LOCKED) => None,
            other => Some(other),
        }?;

        let next = match lock {
            true => Self::LOCKED,
            false => Self::EMPTY,
        };

        match try_swap_ptr(&self.inner, item_ptr, next) {
            true => Some(unsafe { Box::from_raw(item_ptr) }),
            false => None,
        }
    }

    pub fn is_locked(&self) -> bool {
        self.inner.load(SeqCst) == Self::LOCKED
    }

    /// Forcibly unlocks this slot.
    ///
    /// Does nothing if the slot isn't locked
    pub fn unlock(&self) -> bool {
        try_swap_ptr(&self.inner, Self::LOCKED, Self::EMPTY)
    }
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self::NONE
    }
}

impl<T> Drop for Slot<T> {
    fn drop(&mut self) {
        core::mem::drop(self.try_take(true));
    }
}
