use alloc::sync::Arc;

use super::{Fifo, FifoApi};

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
    pub fn fifo<T>() -> Fifo<L, F, T> {
        Fifo::default()
    }

    pub fn arc_fifo<T: 'static>() -> Arc<dyn FifoApi<T>> {
        Arc::new(Self::fifo())
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
