use super::*;

impl<T> Storage<T> for &mut Vec<T> {
    type Output = usize;

    fn reserve(&mut self, len: usize) {
        Vec::reserve(self, len);
    }

    fn push(&mut self, _index: usize, item: T) {
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

    fn push(&mut self, index: usize, item: T) {
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
    pub(super) inner: [Option<T>; N],
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

    fn push(&mut self, index: usize, item: T) {
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

    fn push(&mut self, _index: usize, item: T) {
        *self = Some(item);
    }

    fn bounds(&self) -> (Option<usize>, Option<usize>) {
        (Some(1), Some(1))
    }

    fn finish(self, _pushed: usize) -> Result<Self::Output, Self> {
        self.ok_or(None)
    }
}