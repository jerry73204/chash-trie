mod error;
pub use error::*;

mod node;
mod utils;

use crate::node::Node;
use crossbeam::epoch::{self, Guard};
use error::Error;
use std::borrow::Borrow;
use std::hash::Hash;

#[derive(Debug)]
pub struct Trie<S, V>
where
    S: Eq + Hash,
{
    root: Node<S, V>,
}

impl<S, V> Trie<S, V>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self { root: Node::new() }
    }

    pub fn pin(&self) -> GuardedTrie<'_, S, V> {
        GuardedTrie {
            guard: epoch::pin(),
            trie: self,
        }
    }
}

impl<S, V> Default for Trie<S, V>
where
    S: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct GuardedTrie<'g, S, V>
where
    S: Eq + Hash,
{
    guard: Guard,
    trie: &'g Trie<S, V>,
}

impl<'g, S, V> GuardedTrie<'g, S, V>
where
    S: Eq + Hash,
{
    pub fn get<'a, Q, K>(&self, key: K) -> Option<&V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        self.trie.root.get(key, &self.guard)
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
        self.trie.root.insert(key, value, &self.guard)
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
        let (value, is_child_removed) = self.trie.root.remove(key, &self.guard)?;
        if is_child_removed {
            todo!();
        }

        Ok(value)
    }

    pub fn iter(&'g self) -> Box<dyn Iterator<Item = &'g V> + 'g> {
        self.trie.root.iter(&self.guard)
    }
}
