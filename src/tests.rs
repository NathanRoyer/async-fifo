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
fn test_async() {
    use async_io::block_on;
    use std::thread::spawn;

    async fn sleep_ms_none(millis: u64) -> Option<usize> {
        use core::time::Duration;
        let ten_ms = Duration::from_millis(millis);
        async_io::Timer::after(ten_ms).await;
        None
    }

    let (tx_normal, mut rx_normal) = super::new();

    let total = 500usize;

    let sending_task = async move {
        for i in 1..total {
            let woken_up = tx_normal.send(i);
            assert!(woken_up < 5, "woken_up={woken_up}");
            sleep_ms_none(1).await;
        }
    };

    let receiving_task = async move {
        let mut last = 0;
        while let Ok(item) = rx_normal.recv().await {
            assert_eq!(item, last + 1);
            sleep_ms_none(1).await;
            last = item;
        }
    };

    let thread_a = spawn(|| block_on(sending_task));
    let thread_b = spawn(|| block_on(receiving_task));

    thread_a.join().unwrap();
    thread_b.join().unwrap();
}

#[test]
fn test_cancellation_1() {
    use futures_lite::future::{race, zip};
    use alloc::vec::Vec;

    async fn sleep_ms_none(millis: u64) -> Option<usize> {
        use core::time::Duration;
        let ten_ms = Duration::from_millis(millis);
        async_io::Timer::after(ten_ms).await;
        None
    }

    let (tx_normal, rx_normal) = super::new();
    let rx_fallback = rx_normal.clone();


    let total = 100usize;
    let mut received = Vec::new();

    async_io::block_on(async {
        while received.len() < total {
            let next = received.len();

            let sending_task = async {
                sleep_ms_none(3).await;
                tx_normal.send(next);
            };

            let mut rx_normal = rx_normal.clone();
            let feedback = &mut received;

            let receiving_task = async move {
                let timeout = sleep_ms_none(4);
                let reception = async { rx_normal.recv().await.ok() };

                if let Some(item) = race(timeout, reception).await {
                    feedback.push(item);
                }
            };

            zip(receiving_task, sending_task).await;

            if received.last() != Some(&next) {
                rx_fallback.try_recv().unwrap();
            }
        }
    });

    let expected: Vec<usize> = (0..total).collect();
    assert_eq!(received, expected);
}

#[test]
fn test_cancellation_2() {
    use futures_lite::future::race;
    use async_io::block_on;
    use std::thread::spawn;

    async fn sleep_ms_none(millis: u64) -> Option<usize> {
        use core::time::Duration;
        let ten_ms = Duration::from_millis(millis);
        async_io::Timer::after(ten_ms).await;
        None
    }

    let (tx_normal, rx_normal) = super::new();
    let (tx_feedback, rx_feedback) = super::new();

    let total = 500usize;

    let mut rx_normal_a = rx_normal.clone();
    let tx_feedback_a = tx_feedback.clone();

    let good_citizen_a = async move {
        while let Ok(_item) = rx_normal_a.recv().await {
            // std::println!("A: {}", _item);
            tx_feedback_a.send(0);
            sleep_ms_none(1).await;
        }

        // std::println!("end of good citizen A");
    };

    let mut rx_normal_b = rx_normal.clone();
    let tx_feedback_b = tx_feedback.clone();

    let good_citizen_b = async move {
        while let Ok(_item) = rx_normal_b.recv().await {
            // std::println!("B: {}", _item);
            tx_feedback_b.send(1);
            sleep_ms_none(1).await;
        }

        // std::println!("end of good citizen B");
    };

    let mut rx_normal_c = rx_normal.clone();
    let tx_feedback_c = tx_feedback.clone();

    let bad_citizen_c = async move {
        while !rx_normal_c.no_senders() {
            let timeout = sleep_ms_none(3);
            let reception = async { rx_normal_c.recv().await.ok() };
            if let Some(_item) = race(timeout, reception).await {
                // std::println!("C: {} ----", _item);
                tx_feedback_c.send(2);
            } else {
                // std::println!("C: cancelled");
            }
        }

        // std::println!("end of bad citizen");
    };

    let sending_task = async move {
        for i in 0..total {
            tx_normal.send(i);
            sleep_ms_none(2).await;
        }

        // std::println!("end of sending");
    };

    let thread_a = spawn(|| block_on(good_citizen_a));
    let thread_b = spawn(|| block_on(good_citizen_b));
    let thread_c = spawn(|| block_on(bad_citizen_c));
    let thread_d = spawn(|| block_on(sending_task));

    thread_a.join().unwrap();
    thread_b.join().unwrap();
    thread_c.join().unwrap();
    thread_d.join().unwrap();

    let mut counters = [0; 3];

    for _ in 0..total {
        let i = rx_feedback.try_recv().unwrap();
        counters[i] += 1;
    }

    // std::println!("A got {} items", counters[0]);
    // std::println!("B got {} items", counters[1]);
    // std::println!("C got {} items", counters[2]);
}

#[test]
#[cfg(feature = "blocking")]
fn test_multi_thread_blocking() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    let (producer, mut consumer) = super::new();
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
