use fast_trie::Trie;
use rand::prelude::*;
use std::{
    sync::Arc,
    thread::{sleep, spawn},
    time::Duration,
};

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
            trie.pinned().insert(key.clone(), first_value);

            sleep(Duration::from_millis(10));

            // 15ms
            trie.pinned().insert(key, second_value);
        })
    };

    let reader = spawn(move || {
        // 0ms
        {
            let trie = trie.pinned();
            let value = trie.get(&key);
            assert!(value.is_none());
        }

        sleep(Duration::from_millis(10));

        // 10ms
        {
            let value = *trie.pinned().get(&key).unwrap();
            assert_eq!(value, first_value);
        }

        sleep(Duration::from_millis(10));

        // 20ms
        {
            let value = *trie.pinned().get(&key).unwrap();
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
            trie.pinned().insert(key.clone(), first_value);

            sleep(Duration::from_millis(10));

            // 10ms
            trie.pinned().insert(key, second_value);
        })
    };

    let reader = spawn(move || {
        sleep(Duration::from_millis(5));

        // 5ms
        let pinned = trie.pinned();
        let value = pinned.get(&key).unwrap();
        assert_eq!(*value, first_value);

        sleep(Duration::from_millis(10));

        // 15ms
        assert_eq!(*value, first_value);

        drop(pinned);
        let pinned = trie.pinned();
        let value = pinned.get(&key).unwrap();
        assert_eq!(*value, second_value);
    });

    writer.join().unwrap();
    reader.join().unwrap();
}
