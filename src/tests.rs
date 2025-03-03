#[test]
fn test_one() {
    let (tx, rx) = super::new();

    tx.send("Test");
    let results = rx.try_recv();

    assert_eq!(results, Some("Test"));

    core::mem::drop(tx);
    assert!(rx.no_producers());
}

#[test]
fn test_zero_sized() {
    let (tx, rx) = super::new();
    let array = [(); 16];

    tx.send_iter(array.iter().cloned());
    let results = rx.try_recv_array();

    assert_eq!(results, Some(array));

    core::mem::drop(tx);
    assert!(rx.no_producers());
}

#[test]
fn test_multiple() {
    let (tx, rx) = super::new();

    let to_send: alloc::vec::Vec<_> = (0..12).collect();
    tx.send_iter(to_send.clone().into_iter());

    let mut results = alloc::vec::Vec::new();
    rx.try_recv_many(&mut results);

    assert_eq!(results, to_send);

    core::mem::drop(tx);
    assert!(rx.no_producers());
}

#[test]
fn test_10k() {
    let (tx, rx) = super::new();

    let to_send: alloc::vec::Vec<_> = (0..10000).collect();
    tx.send_iter(to_send.iter().cloned());

    let mut results = alloc::vec::Vec::new();
    rx.try_recv_many(&mut results);

    assert_eq!(results, to_send);

    core::mem::drop(tx);
    assert!(rx.no_producers());
}

#[test]
fn test_multi_steps() {
    use alloc::vec::Vec;

    let (tx, rx) = crate::fifo::BlockSize::<256, 32>::channel();
    let mut results = Vec::new();
    let mut input = Vec::new();

    for _ in 0..8 {
        let to_send: alloc::vec::Vec<_> = (0..100).collect();
        tx.send_iter(to_send.iter().map(|i| *i));
        input.extend(to_send);
        rx.try_recv_many(&mut results);
    }

    assert_eq!(results, input);

    core::mem::drop(tx);
    assert!(rx.no_producers());
}

#[cfg(test)]
fn test_multi_thread_inner() {
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};

    let (producer, consumer) = super::new();
    let total_consumed = Arc::new(AtomicUsize::new(0));

    let sends = 120;
    let total_produced = 12 * sends;
    let to_send: Vec<usize> = (0..100).collect();

    let mut handles = Vec::new();

    for _ in 0..12 {
        let tx = producer.clone();
        let to_send = to_send.clone();

        let thread_fn = move || {
            for _ in 0..sends {
                tx.send_iter(to_send.iter().cloned());
            }
        };

        handles.push(std::thread::spawn(thread_fn));
    }

    for _ in 0..12 {
        let rx = consumer.clone();
        let total_consumed = total_consumed.clone();
        let to_send = to_send.clone();

        let thread_fn = move || {
            while total_consumed.load(Ordering::SeqCst) != total_produced {
                if let Some(array) = rx.try_recv_array::<100>() {
                    assert_eq!(array.as_slice(), &to_send);
                    total_consumed.fetch_add(1, Ordering::SeqCst);
                }
            }
        };

        handles.push(std::thread::spawn(thread_fn));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_multi_thread() {
    for _ in 0..10 {
        test_multi_thread_inner();
    }
}

#[test]
#[cfg(feature = "blocking")]
fn test_multi_thread_blocking() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    let (producer, consumer) = super::new();
    let total_consumed = Arc::new(AtomicUsize::new(0));

    let sends = 120;
    let total_produced = 12 * sends;
    let to_send: Vec<_> = (0..10).collect();

    let mut handles = Vec::new();

    for _ in 0..12 {
        let tx = producer.clone();
        let to_send = to_send.clone();

        let thread_fn = move || {
            for _ in 0..sends {
                tx.send_iter(to_send.clone().into_iter());
            }
        };

        handles.push(std::thread::spawn(thread_fn));
    }

    core::mem::drop(producer);

    while total_consumed.load(Ordering::SeqCst) != total_produced {
        let results = consumer.recv_array_blocking::<10>().expect("closed");
        assert_eq!(results.as_slice(), &to_send);
        total_consumed.fetch_add(1, Ordering::SeqCst);
    }

    assert_eq!(consumer.recv_array_blocking::<10>(), Err(super::Closed));

    for handle in handles {
        let _ = handle.join();
    }
}
