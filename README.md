This crate implements three structures for object transfers:

- an `AtomicSlot<T>` type
- small one-shot SPSC channels
- fully-featured MPMC channels

All of these structures are synchronized without any lock and without spinning/yielding.
This crate is compatible with `no_std` targets.

### `AtomicSlot<T>`

You can atomically swap the contents of this slot from any thread.
Think of it as `Mutex<Option<Box<T>>>` without any actual locking.

### One shot SPSC channels (Async Slots)

```rust
let (tx, rx) = async_fifo::slot::async_slot();
let item = "Hello, world!";
tx.write(Box::new(item));

let _task = async {
    assert_eq!(*rx.await, item);
};
```

This is the asynchronous version of `AtomicSlot<T>`, on which
you can await until an item is inserted. The item has to be boxed.
Very useful for the transfer of return values in the context of remote procedure calling.

### Fully-Featured Channels

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
- Asynchronous and synchronous API, both blocking (todo) and non-blocking
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
