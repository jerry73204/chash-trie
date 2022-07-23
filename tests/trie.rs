use fast_trie::Trie;
use once_cell::sync::Lazy;
use rand::prelude::*;
use std::{
    sync::Arc,
    thread::{available_parallelism, sleep, spawn},
    time::Duration,
};

static NUM_THREADS: Lazy<usize> = Lazy::new(|| available_parallelism().unwrap().get());

#[test]
fn concurrent_reader_writer_test() {
    let trie = Arc::new(Trie::new());

    let mut rng = rand::thread_rng();
    let key: Vec<u8> = [0, 1].choose_multiple(&mut rng, 3).cloned().collect();
    let first_value: u32 = rng.gen();
    let second_value: u32 = loop {
        let sample = rng.gen();
        if sample != first_value {
            break sample;
        }
    };

    let writer = {
        let trie = trie.clone();
        let key = key.clone();

        spawn(move || {
            sleep(Duration::from_millis(5));

            // 5ms
            trie.pin().insert(key.clone(), first_value);

            sleep(Duration::from_millis(10));

            // 15ms
            trie.pin().insert(key, second_value);
        })
    };

    let reader = spawn(move || {
        // 0ms
        {
            let trie = trie.pin();
            let value = trie.get(&key);
            assert!(value.is_none());
        }

        sleep(Duration::from_millis(10));

        // 10ms
        {
            let value = *trie.pin().get(&key).unwrap();
            assert_eq!(value, first_value);
        }

        sleep(Duration::from_millis(10));

        // 20ms
        {
            let value = *trie.pin().get(&key).unwrap();
            assert_eq!(value, second_value);
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn overwrite_test() {
    let trie = Arc::new(Trie::new());

    let mut rng = rand::thread_rng();
    let key: Vec<u8> = [0, 1].choose_multiple(&mut rng, 3).cloned().collect();
    let first_value: u32 = rng.gen();
    let second_value: u32 = loop {
        let sample = rng.gen();
        if sample != first_value {
            break sample;
        }
    };

    let writer = {
        let trie = trie.clone();
        let key = key.clone();

        spawn(move || {
            // 0ms
            trie.pin().insert(key.clone(), first_value);

            sleep(Duration::from_millis(10));

            // 10ms
            trie.pin().insert(key, second_value);
        })
    };

    let reader = spawn(move || {
        sleep(Duration::from_millis(5));

        // 5ms
        let pin = trie.pin();
        let value = pin.get(&key).unwrap();
        assert_eq!(*value, first_value);

        sleep(Duration::from_millis(10));

        // 15ms
        assert_eq!(*value, first_value);

        drop(pin);
        let pin = trie.pin();
        let value = pin.get(&key).unwrap();
        assert_eq!(*value, second_value);
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn race_insert_get_test() {
    let trie = Arc::new(Trie::new());
    let mut rng = rand::thread_rng();

    let key = {
        let mut key = [0u32; 32];
        rng.fill(&mut key);
        key
    };
    let value: u64 = rng.gen();

    let getters: Vec<_> = (0..(*NUM_THREADS - 1))
        .map(|_| {
            let trie = trie.clone();

            spawn(move || {
                let mut success = false;

                for _ in 0..10000 {
                    let curr_value = trie.pin().get(&key).cloned();
                    if let Some(curr_value) = curr_value {
                        assert!(curr_value == value);
                        success = true;
                        break;
                    }
                }

                assert!(success);
            })
        })
        .collect();

    let inserter = spawn(move || {
        sleep(Duration::from_micros(100));
        trie.pin().insert(key, value);
    });

    inserter.join().unwrap();

    for handle in getters {
        handle.join().unwrap();
    }
}
