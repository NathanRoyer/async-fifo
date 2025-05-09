use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::SeqCst;
use core::array::from_fn;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::fifo::{FifoApi, Storage, TmpArray, BlockSize};

/// Fifo Production Handle
///
/// This is the "sending" half of a FIFO channel.
///
/// When you call the methods of this object to send N items,
/// a reservation of N consecutive slots is performed on the
/// underlying FIFO. Then, once this reservation is negociated,
/// all items are pushed into the slots sequentially.
///
/// Unlike [`Consumer`]s, Producers implement [`Clone`].
///
/// # Example
///
/// ```rust
/// let (tx, rx) = async_fifo::new();
/// 
/// // Sending items one by one
/// tx.send('a');
/// tx.send('b');
/// tx.send('c');
/// 
/// // Pouring items from a Vec
/// let mut vec = vec!['a', 'b', 'c'];
/// tx.send_iter(vec.drain(..));
/// 
/// // Sending an array of items
/// let array = ['a', 'b', 'c'];
/// tx.send_iter(array);
/// 
/// // Sending a slice of primitive items
/// let slice = ['a', 'b', 'c'].as_slice();
/// let iter = slice.iter().copied();
/// tx.send_iter(iter);
/// 
/// // Receiving a total of 12 items
/// let expected = ['a', 'b', 'c', 'a', 'b', 'c', 'a', 'b', 'c', 'a', 'b', 'c'];
/// assert_eq!(rx.try_recv_array(), Some(expected));
/// assert_eq!(rx.try_recv(), None);
/// ```
pub struct Producer<T> {
    fifo: Arc<dyn FifoApi<T>>,
    num_prod: Arc<AtomicUsize>,
}

impl<T> Clone for Producer<T> {
    fn clone(&self) -> Self {
        self.num_prod.fetch_add(1, SeqCst);
        Self {
            fifo: self.fifo.clone(),
            num_prod: self.num_prod.clone(),
        }
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        self.num_prod.fetch_sub(1, SeqCst);
    }
}

impl<T> Producer<T> {
    /// Sends a batch of items in the channel, atomically.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send_iter<I>(&self, into_iter: I)
    where
        I: IntoIterator,
        I::IntoIter: ExactSizeIterator<Item = T>,
    {
        self.fifo.push(&mut into_iter.into_iter());
    }

    /// Sends one item through the channel.
    ///
    /// This operation is non-blocking and always succeeds immediately.
    pub fn send(&self, item: T) {
        self.send_iter(core::iter::once(item));
    }
}

/// Fifo Consumption Handle
///
/// This handle allows you to pull items or batches
/// of items from the FIFO, atomically.
///
/// Under the hood, most methods call [`Self::try_recv_into`],
/// which knows how many items to pull from the FIFO based on
/// the constraints set by its [`Storage`] parameter. For instance,
/// [`Self::try_recv_exact`] has the effect of pulling either
/// zero or exactly N items, depending on how many items are
/// available in the FIFO.
///
/// Items are pulled in the exact same order as they were pushed.
#[derive(Clone)]
pub struct Consumer<T> {
    fifo: Arc<dyn FifoApi<T>>,
    num_prod: Arc<AtomicUsize>,
}

impl<T> Consumer<T> {
    /// Returns true if all producers have been dropped
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, rx) = async_fifo::new();
    /// tx.send('z');
    ///
    /// // one remaining producer
    /// assert_eq!(rx.no_producers(), false);
    ///
    /// // drop it
    /// core::mem::drop(tx);
    ///
    /// // all producers are gone
    /// assert_eq!(rx.no_producers(), true);
    ///
    /// // No producer, yes, but one item is still in there.
    /// assert_eq!(rx.try_recv(), Some('z'));
    /// assert_eq!(rx.try_recv(), None);
    ///
    /// ```
    pub fn no_producers(&self) -> bool {
        self.num_prod.load(SeqCst) == 0
    }

    /// Tries to receive one item.
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    /// 
    /// // Receive one by one
    /// assert_eq!(rx.try_recv(), Some('a'));
    /// assert_eq!(rx.try_recv(), Some('b'));
    /// assert_eq!(rx.try_recv(), Some('c'));
    /// assert_eq!(rx.try_recv(), None);
    /// ```
    pub fn try_recv(&self) -> Option<T> {
        self.try_recv_array().map(|[item]| item)
    }

    /// Tries to receive as many items as possible, into a vector.
    ///
    /// If at least one item is received, the number of
    /// received items is returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c', 'd']);
    /// 
    /// // Pull as much as possible into a vec
    /// let mut bucket = Vec::new();
    /// assert_eq!(rx.try_recv_many(&mut bucket), Some(4));
    /// assert_eq!(bucket, ['a', 'b', 'c', 'd']);
    ///
    /// assert_eq!(rx.try_recv(), None);
    /// ```
    pub fn try_recv_many(&self, vec: &mut Vec<T>) -> Option<usize> {
        self.try_recv_into(vec).ok()
    }

    /// Tries to receive exactly `slice.len()` items into a slice.
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    /// 
    /// // Pull a specific amount into a slice
    /// let mut buffer = ['_'; 3];
    /// assert!(rx.try_recv_exact(&mut buffer).is_some());
    /// assert_eq!(buffer, ['a', 'b', 'c']);
    /// assert_eq!(rx.try_recv(), None);
    /// ```
    pub fn try_recv_exact(&self, slice: &mut [T]) -> Option<()> {
        self.try_recv_into(slice).ok()
    }

    /// Tries to receive exactly `N` items into an array.
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, rx) = async_fifo::new();
    /// tx.send_iter(['a', 'b', 'c']);
    /// 
    /// // Pull a specific amount into an array
    /// assert_eq!(rx.try_recv_array(), Some(['a', 'b', 'c']));
    /// assert_eq!(rx.try_recv(), None);
    /// ```
    pub fn try_recv_array<const N: usize>(&self) -> Option<[T; N]> {
        let array = TmpArray {
            inner: from_fn(|_| None),
        };

        self.try_recv_into(array).ok()
    }

    /// Tries to receive some items into custom storage.
    pub fn try_recv_into<S: Storage<T>>(&self, mut storage: S) -> Result<S::Output, S> {
        let pushed = self.fifo.pull(&mut storage);
        storage.finish(pushed)
    }

    pub(crate) fn fifo(&self) -> &dyn FifoApi<T> {
        &*self.fifo
    }
}

impl<const L: usize, const F: usize> BlockSize<L, F> {
    pub fn non_blocking<T: 'static>() -> (Producer<T>, Consumer<T>) {
        let fifo = Self::arc_fifo();
        let num_prod = Arc::new(AtomicUsize::new(1));

        let producer = Producer {
            fifo: fifo.clone(),
            num_prod: num_prod.clone(),
        };

        let consumer = Consumer {
            fifo,
            num_prod,
        };

        (producer, consumer)
    }
}
