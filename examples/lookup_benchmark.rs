use chash_trie::Trie;
use clap::Parser;
use rand::prelude::*;
use rayon::prelude::*;
use std::{
    sync::Arc,
    thread::{available_parallelism, spawn},
    time::Instant,
};

#[derive(Parser)]
struct Opts {
    #[clap(long, default_value_t = available_parallelism().unwrap().get())]
    pub num_threads: usize,
    #[clap(long)]
    pub num_words: usize,
    #[clap(long)]
    pub num_lookups: usize,
    #[clap(long)]
    pub max_word_bytes: usize,
}

fn main() {
    let Opts {
        num_threads,
        num_lookups,
        num_words,
        max_word_bytes,
    } = Opts::parse();
    assert!(max_word_bytes >= 1);

    let mut rng = rand::thread_rng();

    println!("Generating key/value pairs");

    let dictionary: Vec<_> = {
        (0..num_words)
            .map(|_| {
                let len: usize = rng.gen_range(1..=max_word_bytes);
                let key: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                let value: u64 = rng.gen();
                (key, value)
            })
            .collect()
    };

    println!("Building the trie concurrently");
    let trie = Arc::new(Trie::new());

    dictionary.into_par_iter().for_each(|(key, value)| {
        trie.pin().insert(key, value);
    });

    println!("Run concurrent lookups");
    let since = Instant::now();
    let threads: Vec<_> = (0..num_threads)
        .map(|_| {
            let trie = trie.clone();

            spawn(move || {
                let mut rng = rand::thread_rng();

                for _ in 0..num_lookups {
                    let trie = trie.pin();
                    let len: usize = rng.gen_range(1..=max_word_bytes);
                    let key: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                    let _value = trie.get(&key);
                }
            })
        })
        .collect();

    for handle in threads {
        handle.join().unwrap();
    }

    println!("elapsed {:?}", since.elapsed());
}
