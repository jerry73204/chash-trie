mod entry;
mod error;
use entry::Entry;
pub use error::*;

mod node;

use crate::node::Node;
use crossbeam::epoch::{self, Atomic, Guard, Owned, Shared};
use error::Error;
use std::borrow::Borrow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash};
use std::sync::atomic::Ordering::*;

#[derive(Debug)]
pub struct Trie<S, V, H = RandomState> {
    root: Atomic<Node<S, V, H>>,
    hash_builder: H,
}

impl<S, V, H> Trie<S, V, H>
where
    S: Eq + Hash,
    H: BuildHasher + Clone,
{
    pub fn with_hasher(hash_builder: H) -> Self {
        Self {
            root: Atomic::null(),
            hash_builder,
        }
    }

    pub fn pin(&self) -> GuardedTrie<'_, S, V, H> {
        GuardedTrie {
            guard: epoch::pin(),
            trie: self,
        }
    }
}

impl<S, V> Trie<S, V, RandomState>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self::with_hasher(RandomState::default())
    }
}

impl<S, V> Default for Trie<S, V, RandomState>
where
    S: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct GuardedTrie<'g, S, V, H> {
    guard: Guard,
    trie: &'g Trie<S, V, H>,
}

impl<'g, S, V, H> GuardedTrie<'g, S, V, H>
where
    S: Eq + Hash,
    H: BuildHasher + Clone,
{
    pub fn get<'a, Q, K>(&self, key: K) -> Option<&V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        self.root()?.get(key, self)
    }

    pub fn insert<K>(&self, key: K, value: V) -> Option<&V>
    where
        K: IntoIterator<Item = S> + Clone,
        V: Clone,
    {
        loop {
            match self.try_insert(key.clone(), value.clone()) {
                Ok(value) => break Some(value),
                Err(Error::NotFound) => break None,
                Err(Error::Retry) => (),
            }
        }
    }

    pub fn try_insert<K>(&self, key: K, value: V) -> Result<&V, Error>
    where
        K: IntoIterator<Item = S>,
    {
        self.get_or_create_root().insert(key, value, self)
    }

    pub fn remove<'a, Q, K>(&self, key: K) -> Option<&V>
    where
        K: IntoIterator<Item = &'a Q> + Clone,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        loop {
            match self.try_remove(key.clone()) {
                Ok(value) => break Some(value),
                Err(Error::NotFound) => break None,
                Err(Error::Retry) => {}
            }
        }
    }

    pub fn try_remove<'a, Q, K>(&self, key: K) -> Result<&V, Error>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let (value, is_child_removed) = self.root().ok_or(Error::NotFound)?.remove(key, self)?;

        if is_child_removed {
            self.trie.root.store(Shared::null(), Release);
        }

        Ok(value)
    }

    pub fn iter(&'g self) -> Box<dyn Iterator<Item = &'g V> + 'g> {
        Box::new(self.root().into_iter().flat_map(|root| root.iter(self)))
    }

    pub fn entry<'a, Q, K>(&'g self, key: K) -> Option<Entry<'g, S, V, H>>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let node = self.root()?.find(key, self)?;
        Some(Entry { node, trie: self })
    }

    fn root(&self) -> Option<&Node<S, V, H>> {
        let shared = self.trie.root.load_consume(&self.guard);
        unsafe { shared.as_ref() }
    }

    fn get_or_create_root(&self) -> &Node<S, V, H> {
        match self.root() {
            Some(root) => root,
            None => {
                let new_shared = Owned::new(Node::new()).into_shared(&self.guard);
                let result = self.trie.root.compare_exchange(
                    Shared::null(),
                    new_shared,
                    AcqRel,
                    Acquire,
                    &self.guard,
                );

                let shared = match result {
                    Ok(_) => new_shared,
                    Err(error) => error.current,
                };
                unsafe { shared.deref() }
            }
        }
    }
}
