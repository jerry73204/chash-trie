use clap::Parser;
use std::{
    sync::{Arc, Mutex},
    thread::{available_parallelism, spawn},
    time::Instant,
};

#[derive(Parser)]
struct Opts {
    #[clap(long, default_value_t = available_parallelism().unwrap().get())]
    pub num_threads: usize,
    #[clap(long)]
    pub num_rounds: usize,
}

fn main() {
    let opts = Opts::parse();

    let since = Instant::now();
    run_trie_rs(opts.num_threads);
    println!("trie_rs {:?}", since.elapsed());

    let since = Instant::now();
    run_fast_trie(opts.num_rounds, opts.num_threads);
    println!("fast-trie {:?}", since.elapsed());
}

fn run_fast_trie(num_rounds: usize, num_threads: usize) {
    use fast_trie::Trie;

    let trie = Arc::new(Trie::new());

    let threads: Vec<_> = (0..num_threads)
        .map(|worker_id| {
            let trie = trie.clone();

            spawn(move || {
                let trie = trie.pin();

                (0..num_rounds).for_each(|round_id| {
                    trie.insert([worker_id, round_id], 0);
                });
            })
        })
        .collect();

    for handle in threads {
        handle.join().unwrap();
    }
}

fn run_trie_rs(num_rounds: usize) {
    use trie_rs::TrieBuilder;

    let builder = Arc::new(Mutex::new(TrieBuilder::new()));
    let n_workers = available_parallelism().unwrap().get();

    let threads: Vec<_> = (0..n_workers)
        .map(|worker_id| {
            let builder = builder.clone();

            spawn(move || {
                (0..num_rounds).for_each(|round_id| {
                    builder.lock().unwrap().push([worker_id, round_id]);
                });
            })
        })
        .collect();

    for handle in threads {
        handle.join().unwrap();
    }

    let builder = Arc::try_unwrap(builder)
        .unwrap_or_else(|_| unreachable!())
        .into_inner()
        .unwrap();
    builder.build();
}
