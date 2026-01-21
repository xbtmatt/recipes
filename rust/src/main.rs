fn main() {
    println!("Throttled receiver pattern below!");
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use tokio::{
        sync::watch,
        time::{Duration, Instant, sleep},
    };

    const TOLERANCE_MS: u128 = 20;

    fn assert_within_tolerance(actual: u128, expected: u128, label: &str) {
        let diff = (actual as i128 - expected as i128).unsigned_abs();
        assert!(
            diff <= TOLERANCE_MS,
            "{} was {}ms, expected {}ms (+/- {}ms)",
            label,
            actual,
            expected,
            TOLERANCE_MS
        );
    }

    /// Tests that [`tokio::sync::watch`] behaves as expected for the throttled receiver pattern
    /// used in the [`crate::main`] function.
    #[tokio::test(flavor = "current_thread")]
    async fn throttle_outputs_expected_messages() {
        let (tx, rx) = watch::channel(("".to_string(), 0u128));
        let start = Instant::now();
        let received = Rc::new(RefCell::new(Vec::<(String, u128, u128)>::new()));

        // The (start_time, message) tuple sent to the receiver.
        // That is, "a" is sent at ~0ms, "b" is sent at ~600ms, "c" is sent at ~1200ms, etc.
        // For a receiver that only prints the latest message every second, this results in an
        // output pattern where "a" and "b" are both printed, but "c" is skipped because "d" occurs
        // before the second time boundary (2000ms).
        // The rest of the messages test that the last message is still printed even if nothing
        // explicitly triggers the handler after the third time boundary, since there are no
        // messages after 3000ms.
        let pairs = [
            (0, "a"),
            (600, "b"),
            (1200, "c"),
            (1800, "d"),
            (2050, "e"),
            (2075, "f"),
            (2100, "g"),
            (2150, "h"),
            (2200, "i"),
        ];

        let received_clone = received.clone();

        assert!(pairs.is_sorted());
        // Wait a little over a second past the last message's sent time to ensure that
        // the receiver task wraps up properly.
        let wrap_up_time = pairs.last().unwrap().0 + 1100;

        tokio::select! {
            _ = async {
                let mut last_time = 0u64;
                for (time, msg) in pairs {
                    sleep(Duration::from_millis(time - last_time)).await;
                    let elapsed = start.elapsed().as_millis();
                    let _ = tx.send((msg.to_string(), elapsed));
                    last_time = time;
                }
                sleep(Duration::from_millis(wrap_up_time)).await;
            } => {},
            _ = throttled_receiver(rx, received_clone, start) => {},
        }

        let result = received.borrow();

        // The expected values are a 3-tuple of:
        // (msg, sent_at, read_at).
        let expected: [(&str, u128, u128); 4] = [
            ("a", 0, 0),
            ("b", 600, 1000),
            ("d", 1800, 2000),
            ("i", 2200, 3000),
        ];

        assert_eq!(result.len(), expected.len());
        for (result, expected) in result.iter().zip(expected.iter()) {
            let (msg, sent_at, read_at) = result.clone();
            let (expected_msg, expected_sent_at, expected_read_at) = *expected;
            assert_eq!(msg, expected_msg);
            assert_within_tolerance(sent_at, expected_sent_at, &format!("'{msg}' sent at"));
            assert_within_tolerance(read_at, expected_read_at, &format!("'{msg}' read at"));
        }
    }

    async fn throttled_receiver(
        mut rx: watch::Receiver<(String, u128)>,
        output: Rc<RefCell<Vec<(String, u128, u128)>>>,
        start: Instant,
    ) {
        const THROTTLE_MS: u64 = 1000;

        loop {
            rx.changed().await.unwrap();
            // Differentiate between when the message was read vs sent. `sent_at` is when the
            // message is sent by the receiver, `read_at` is `rx.changed()` stops blocking and
            // reads the value.
            let read_at = start.elapsed().as_millis();
            let (msg, sent_at) = rx.borrow().clone();
            output.borrow_mut().push((msg, sent_at, read_at));
            sleep(Duration::from_millis(THROTTLE_MS)).await;
        }
    }
}
