use core::array::from_fn;
use core::task::Waker;

use crate::slot::AtomicSlot;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::block::{Fifo, FifoImpl};
use super::storage::TmpArray;
use super::Storage;

#[derive(Default)]
/// Custom Block Size
///
/// In channels, items are stored in contiguous blocks (chunks) of item slots.
/// New blocks are allocated as items are send through the channel.
/// A large block size will result in less but bigger allocations.
/// A small block size will result in more, smaller allocations.
/// For channels transporting large amounts of items, a large block size is preferred.
/// This structure allows you to create channels with custom block size (chunk length).
///
/// See the following pre-defined block sizes:
/// - [`SmallBlockSize`]: 8 slots per block
/// - [`DefaultBlockSize`]: 32 slots per block
/// - [`LargeBlockSize`]: 4096 slots per block
/// - [`HugeBlockSize`]: 1048576 slots per block
///
/// L must be equal to F x 8
pub struct BlockSize<const L: usize, const F: usize>;

impl<const L: usize, const F: usize> BlockSize<L, F> {
    fn build_n<T: 'static>(num_rx: usize) -> (Producer<T>, Arc<dyn FifoImpl<T>>) {
        assert_eq!(F * 8, L);
        let wakers = (0..num_rx).map(|_| AtomicSlot::default()).collect();
        let fifo: Fifo<L, F, T> = Fifo::new(Vec::into(wakers));

        let arc = Arc::new(fifo);

        let producer = Producer {
            fifo: arc.clone(),
        };

        (producer, arc)
    }

    /// Creates a Fifo with this block size.
    ///
    /// `N` is the number of consumers.
    ///
    /// Panics if `F` isn't equal to `L / 8`;
    pub fn build<const N: usize, T: 'static>() -> (Producer<T>, [Consumer<T>; N]) {
        let (tx, fifo) = Self::build_n::<T>(N);

        let rx_array = from_fn(|i| Consumer {
            fifo: fifo.clone(),
            waker_index: i,
        });

        (tx, rx_array)
    }

    pub fn build_vec<T: 'static>(num_rx: usize) -> (Producer<T>, Vec<Consumer<T>>) {
        let (tx, fifo) = Self::build_n::<T>(num_rx);
        let mut rx_vec = Vec::with_capacity(num_rx);

        for i in 0..num_rx {
            rx_vec.push(Consumer {
                fifo: fifo.clone(),
                waker_index: i,
            })
        }

        (tx, rx_vec)
    }

    pub fn build_box<const N: usize, T: 'static>() -> (Producer<T>, Box<[Consumer<T>; N]>) {
        let (tx, rx_vec) = Self::build_vec::<T>(N);
        match Box::try_from(rx_vec) {
            Ok(rx_box) => (tx, rx_box),
            Err(_) => unreachable!()
        }
    }
}

/// Block size suitable for sending items one by one, from time to time
///
/// Each block will have 8 slots.
pub type SmallBlockSize = BlockSize<8, 1>;

/// Reasonable default block size
///
/// Each block will have 32 slots.
pub type DefaultBlockSize = BlockSize<32, 4>;

/// Block size suitable for batch sending of many items
///
/// Each block will have 4096 slots.
pub type LargeBlockSize = BlockSize<4096, 512>;

/// Block size suitable for batch sending of tons of items
///
/// Each block will have 1 048 576 slots.
pub type HugeBlockSize = BlockSize<1048576, 131072>;

/// Creates a Fifo with default block size (Array of Consumers)
pub fn new<const N: usize, T: 'static>() -> (Producer<T>, [Consumer<T>; N]) {
    DefaultBlockSize::build()
}

/// Creates a Fifo with default block size (Vec of Consumers)
pub fn new_vec<T: 'static>(num_consumers: usize) -> (Producer<T>, Vec<Consumer<T>>) {
    DefaultBlockSize::build_vec::<T>(num_consumers)
}

/// Creates a Fifo with default block size (Boxed Array of Consumers)
pub fn new_box<const N: usize, T: 'static>() -> (Producer<T>, Box<[Consumer<T>; N]>) {
    DefaultBlockSize::build_box::<N, T>()
}

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
/// let (tx, [rx]) = async_fifo::new();
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
    fifo: Arc<dyn FifoImpl<T>>,
}

impl<T> Clone for Producer<T> {
    fn clone(&self) -> Self {
        let fifo = self.fifo.clone();
        fifo.inc_num_prod();
        Self {
            fifo,
        }
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        self.fifo.dec_num_prod();
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
        let mut iter = into_iter.into_iter();
        self.fifo.send_iter(&mut iter);
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
pub struct Consumer<T> {
    pub(super) fifo: Arc<dyn FifoImpl<T>>,
    waker_index: usize,
}

unsafe impl<T> Send for Producer<T> {}
unsafe impl<T> Sync for Producer<T> {}

unsafe impl<T> Send for Consumer<T> {}
unsafe impl<T> Sync for Consumer<T> {}

impl<T> Consumer<T> {
    /// Returns true if all producers have been dropped
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, [rx]) = async_fifo::new();
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
        self.fifo.no_producers()
    }

    #[doc(hidden)]
    pub fn is_closed(&self) -> bool {
        self.no_producers()
    }

    /// Tries to receive one item.
    ///
    /// # Example
    ///
    /// ```rust
    /// let (tx, [rx]) = async_fifo::new();
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
    /// let (tx, [rx]) = async_fifo::new();
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
    /// let (tx, [rx]) = async_fifo::new();
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
    /// let (tx, [rx]) = async_fifo::new();
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
        let pushed = self.fifo.try_recv(&mut storage);
        storage.finish(pushed)
    }

    /// Sets the waker of the current task, to be woken up when new items are available.
    pub fn insert_waker(&self, waker: Box<Waker>) {
        self.fifo.insert_waker(waker, self.waker_index);
    }

    /// Tries to take back a previously inserted waker.
    pub fn take_waker(&self) -> Option<Box<Waker>> {
        self.fifo.take_waker(self.waker_index)
    }
}
