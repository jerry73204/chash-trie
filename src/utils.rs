use dashmap::DashMap;
use fnv::FnvBuildHasher;
use once_cell::sync::Lazy;
use std::hash::Hash;
use std::thread::available_parallelism;

pub type Map<K, V> = DashMap<K, V, FnvBuildHasher>;

static DEFAULT_SHARD_AMOUNT: Lazy<usize> =
    Lazy::new(|| (available_parallelism().map_or(1, usize::from) * 4).next_power_of_two());

pub fn new_map<K, V>() -> DashMap<K, V, FnvBuildHasher>
where
    K: Hash + Eq,
{
    DashMap::with_capacity_and_hasher_and_shard_amount(
        0,
        FnvBuildHasher::default(),
        *DEFAULT_SHARD_AMOUNT,
    )
}
