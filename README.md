This crate implements three lock-free structures for object transfers:

- fully-featured MPMC channels
- small one-shot SPSC channels
- an `AtomicSlot<T>` type

All of these structures are synchronized without any locks and without spinning/yielding.
This crate is compatible with `no_std` targets, except for the `_blocking` methods.

### Fully-featured MPMC Channels

```rust
// We'll request two consumers
let (tx, [mut rx1, mut rx2]) = async_fifo::new();

// sending an item
let item = "Hello, world!";
tx.send(item);

// receiving it through the second consumer
assert_eq!(rx2.try_recv(), Some(item));

// producers can be cloned
let _tx2 = tx.clone();

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
- Producers can be cloned (but not consumers)
- The number of consumers must be provided to the constructor

Note: all items under [`fifo`] are quietly re-exported at the root of the crate.

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

### `AtomicSlot<T>`

You can atomically swap the contents of this slot from any thread.
Think of it as `Mutex<Option<Box<T>>>` without any actual locking.
