This crate implements three lock-free structures for object transfers:

- fully-featured MPMC channels
- small one-shot SPSC channels
- an `AtomicSlot<T>` type

All of these structures are synchronized without any locks and without spinning/yielding.
This crate is compatible with `no_std` targets, except for the `_blocking` methods.

See the complete documentation: https://docs.rs/async-fifo/
