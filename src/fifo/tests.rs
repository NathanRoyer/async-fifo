#[test]
fn test_one() {
    use core::iter::once;
    use alloc::vec;
    let (tx, [rx]) = super::new();

    tx.send_iter(once("Test"));
    let results = rx.try_recv_many();

    assert_eq!(results, vec!["Test"]);
}

#[test]
fn test_many() {
    let (tx, [rx]) = super::new();

    let to_send: alloc::vec::Vec<_> = (0..12).collect();
    tx.send_iter(to_send.clone().into_iter());

    let results = rx.try_recv_many();

    assert_eq!(results, to_send);
}

#[test]
fn test_awful_lot() {
    let (tx, [rx]) = super::new();

    let to_send: alloc::vec::Vec<_> = (0..10000).collect();
    tx.send_iter(to_send.iter().cloned());

    let results = rx.try_recv_many();

    assert_eq!(results, to_send);
}

#[test]
fn test_multi_steps() {
    use alloc::vec::Vec;
    let (tx, [rx]) = super::with_block_size::<1, 256, 32, _>();
    let mut results = Vec::new();
    let mut input = Vec::new();

    for _ in 0..8 {
        let to_send: alloc::vec::Vec<_> = (0..100).collect();
        tx.send_iter(to_send.clone().into_iter());
        input.extend(to_send);
        results.extend(rx.try_recv_many());
    }

    assert_eq!(results, input);
}

#[cfg(test)]
fn test_multi_thread_inner() {
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};

    let (producer, consumers) = super::new::<12, usize>();
    let mut consumers = Vec::from(consumers);
    let total_consumed = Arc::new(AtomicUsize::new(0));

    let sends = 120;
    let total_produced = 12 * sends;
    let to_send: Vec<_> = (0..100).collect();

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

    for _ in 0..consumers.len() {
        let rx = consumers.remove(0);
        let total_consumed = total_consumed.clone();
        let to_send = to_send.clone();

        let thread_fn = move || {
            while total_consumed.load(Ordering::SeqCst) != total_produced {
                if let Some(array) = rx.try_recv_exact::<100>() {
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
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};

    let (producer, [consumer]) = super::new();
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

    while total_consumed.load(Ordering::SeqCst) != total_produced {
        let results = consumer.recv_exact_blocking::<10>();
        assert_eq!(results.as_slice(), &to_send);
        total_consumed.fetch_add(1, Ordering::SeqCst);
    }

    for handle in handles {
        let _ = handle.join();
    }
}
