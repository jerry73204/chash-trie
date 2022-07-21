mod node;

use crate::node::Node;
use crossbeam::epoch::{self, Guard};
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

    pub fn pinned(&self) -> PinnedTrie<'_, S, V> {
        PinnedTrie {
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
pub struct PinnedTrie<'g, S, V>
where
    S: Eq + Hash,
{
    guard: Guard,
    trie: &'g Trie<S, V>,
}

impl<'g, S, V> PinnedTrie<'g, S, V>
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
        K: IntoIterator<Item = S>,
    {
        self.trie.root.insert(key, value, &self.guard)
    }

    pub fn remove<'a, Q, K>(&self, key: K) -> Option<&V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        self.trie.root.remove(key, None, &self.guard)
    }

    pub fn iter(&'g self) -> Box<dyn Iterator<Item = &'g V> + 'g> {
        self.trie.root.iter(&self.guard)
    }
}
