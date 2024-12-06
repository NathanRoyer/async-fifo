#[test]
fn test_one() {
    use core::iter::once;
    use alloc::vec;
    let ([mut tx], [mut rx]) = super::new();

    tx.send_iter(once("Test"));
    let results = rx.try_recv_many();

    assert_eq!(results, vec!["Test"]);
}

#[test]
fn test_many() {
    let ([mut tx], [mut rx]) = super::new();

    let to_send: alloc::vec::Vec<_> = (0..12).collect();
    tx.send_iter(to_send.clone().into_iter());

    let results = rx.try_recv_many();

    assert_eq!(results, to_send);
}

#[test]
fn test_awful_lot() {
    let ([mut tx], [mut rx]) = super::new();

    let to_send: alloc::vec::Vec<_> = (0..10000).collect();
    tx.send_iter(to_send.clone().into_iter());

    let results = rx.try_recv_many();

    assert_eq!(results, to_send);
}

#[test]
fn test_multi_steps() {
    use alloc::vec::Vec;
    let ([mut tx], [mut rx]) = super::with_block_size::<1, 1, 256, 32, _>();
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

#[test]
fn test_multi_thread() {
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};

    let (producers, consumers): ([_; 12], [_; 12]) = super::new();
    let mut producers = Vec::from(producers);
    let mut consumers = Vec::from(consumers);
    let total_consumed = Arc::new(AtomicUsize::new(0));

    let sends = 120;
    let total_produced = producers.len() * sends;
    let to_send: Vec<_> = (0..10).collect();

    let mut handles = Vec::new();

    for _ in 0..producers.len() {
        let mut tx = producers.remove(0);
        let to_send = to_send.clone();

        let thread_fn = move || {
            for _ in 0..sends {
                tx.send_iter(to_send.clone().into_iter());
            }
        };

        handles.push(std::thread::spawn(thread_fn));
    }

    for _ in 0..consumers.len() {
        let mut rx = consumers.remove(0);
        let to_send = to_send.clone();
        let total_consumed = total_consumed.clone();

        let thread_fn = move || {
            while total_consumed.load(Ordering::SeqCst) != total_produced {
                if let Some(results) = rx.try_recv_exact::<10>() {
                    assert_eq!(results.as_slice(), &to_send);
                    total_consumed.fetch_add(1, Ordering::SeqCst);
                }
            }
        };

        handles.push(std::thread::spawn(thread_fn));
    }

    for handle in handles {
        let _ = handle.join();
    }
}
