This crate implements three lock-free structures for object transfers:

- an `AtomicSlot<T>` type
- small one-shot SPSC channels
- fully-featured MPMC channels

All of these structures are synchronized without any locks and without spinning/yielding.
This crate is compatible with `no_std` targets, except for the `_blocking` methods.

### `AtomicSlot<T>`

You can atomically swap the contents of this slot from any thread.
Think of it as `Mutex<Option<Box<T>>>` without any actual locking.

### One shot SPSC channels

```rust
let (tx, rx) = async_fifo::slot::oneshot();
let item = "Hello, world!";
tx.send(Box::new(item));

let _task = async {
    assert_eq!(*rx.await, item);
};
```

Very simple single-use channels with a sync and async API, blocking/non-blocking.
This builds on `AtomicSlot<T>`, so the item has to be boxed.
Most useful for the transfer of return values in the context of remote procedure calling, as "reply pipes".

### Fully-featured MPMC Channels

```rust
let ([mut tx1, tx2], [mut rx1, mut rx2]) = async_fifo::fifo::new();
let item = "Hello, world!";
tx1.send(item);
assert_eq!(rx2.try_recv(), Some(item));

// asynchronous use
let _task = async {
    loop {
        println!("Received: {:?}", rx1.recv().await);
    }
};
```

These channels have the following characteristics:

- Multi-Producer / Multi-Consumer (MPMC) with every item routed to exactly one consumer
- Asynchronous and synchronous API, both blocking and non-blocking
- Strict delivery order: these channels have strong FIFO guarantees
- Batch production and consumption (both atomic)
- Send operations are guaranteed to succeed immediately without any sort of yielding/blocking

#### Internal Details

Fifos internally own a chain of blocks.

- blocks have a number of item slots (32 by default)
- they are allocated and appended to the chain by producers
- they are then populated by producers and consumed by consumers
- fully consumed first blocks get recycled and appended again at the end of the chain (when no one is visiting them anymore)
- the fifo has atomic cursors to track which block slots are available for production and consumption
