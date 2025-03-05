use alloc::vec::Vec;

/// Dyn-Compatible subset of [`Storage`] trait
///
/// Automatically derived for every Storage implementor.
pub trait InternalStorageApi<T> {
    fn bounds(&self) -> (Option<usize>, Option<usize>);
    fn insert(&mut self, index: usize, item: T);
    fn reserve(&mut self, len: usize);
}

/// Backing storage for pull operations
pub trait Storage<T>: Sized {
    type Output;

    /// Insert an item into a storage slot
    ///
    /// `index` is the index of this push operation
    /// since the call to `Consumer::try_recv_*`.
    fn insert(&mut self, index: usize, item: T);

    /// Called to seal the storage after all negociated
    /// items have been received
    ///
    /// The negociated number of items is available as `pushed`.
    fn finish(self, pushed: usize) -> Result<Self::Output, Self>;

    #[allow(unused_variables)]
    /// Pre-allocate space for `len` additional items
    fn reserve(&mut self, len: usize) {}

    /// Specify the number of items that this storage
    /// can handle
    ///
    /// The value must be returned as (minimum, maximum).
    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (None, None)
    }
}

impl<T, S: Storage<T>> InternalStorageApi<T> for S {
    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        S::bounds(self)
    }

    fn insert(&mut self, index: usize, item: T) {
        S::insert(self, index, item)
    }

    fn reserve(&mut self, len: usize) {
        S::reserve(self, len)
    }
}

impl<T> Storage<T> for &mut Vec<T> {
    type Output = usize;

    fn reserve(&mut self, len: usize) {
        Vec::reserve(self, len);
    }

    fn insert(&mut self, _index: usize, item: T) {
        Vec::push(self, item);
    }

    fn finish(self, pushed: usize) -> Result<Self::Output, Self> {
        match pushed {
            0 => Err(self),
            _ => Ok(pushed),
        }
    }
}

impl<T> Storage<T> for &mut [T] {
    type Output = ();

    fn insert(&mut self, index: usize, item: T) {
        self[index] = item;
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(self.len()), Some(self.len()))
    }

    fn finish(self, pushed: usize) -> Result<Self::Output, Self> {
        match pushed {
            0 => Err(self),
            _ => Ok(()),
        }
    }
}

#[doc(hidden)]
pub struct TmpArray<const N: usize, T> {
    pub inner: [Option<T>; N],
}

impl<const N: usize, T> Default for TmpArray<N, T> {
    fn default() -> Self {
        Self { 
            inner: core::array::from_fn(|_| None),
        }
    }
}

impl<const N: usize, T> Storage<T> for TmpArray<N, T> {
    type Output = [T; N];

    fn insert(&mut self, index: usize, item: T) {
        self.inner[index] = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(N), Some(N))
    }

    fn finish(self, pushed: usize) -> Result<Self::Output, Self> {
        match pushed {
            0 => Err(self),
            _ => Ok(self.inner.map(Option::unwrap)),
        }
    }
}

impl<T> Storage<T> for Option<T> {
    type Output = T;

    fn insert(&mut self, _index: usize, item: T) {
        *self = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(1), Some(1))
    }

    fn finish(self, _pushed: usize) -> Result<Self::Output, Self> {
        self.ok_or(None)
    }
}
