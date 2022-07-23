mod error;
pub use error::*;

mod node;
mod utils;

use crate::node::Node;
use crossbeam::epoch::{self, Atomic, Guard, Owned, Shared};
use error::Error;
use std::borrow::Borrow;
use std::hash::Hash;
use std::sync::atomic::Ordering::*;

#[derive(Debug)]
pub struct Trie<S, V>
where
    S: Eq + Hash,
{
    root: Atomic<Node<S, V>>,
}

impl<S, V> Trie<S, V>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            root: Atomic::null(),
        }
    }

    pub fn pin(&self) -> GuardedTrie<'_, S, V> {
        GuardedTrie {
            guard: epoch::pin(),
            root: &self.root,
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
    root: &'g Atomic<Node<S, V>>,
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
        self.root()?.get(key, &self.guard)
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
        self.get_or_create_root().insert(key, value, &self.guard)
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
        let (value, is_child_removed) = self
            .root()
            .ok_or(Error::NotFound)?
            .remove(key, &self.guard)?;

        if is_child_removed {
            self.root.store(Shared::null(), Release);
        }

        Ok(value)
    }

    pub fn iter(&'g self) -> Box<dyn Iterator<Item = &'g V> + 'g> {
        Box::new(
            self.root()
                .into_iter()
                .flat_map(|root| root.iter(&self.guard)),
        )
    }

    fn root(&self) -> Option<&Node<S, V>> {
        let shared = self.root.load_consume(&self.guard);
        unsafe { shared.as_ref() }
    }

    fn get_or_create_root(&self) -> &Node<S, V> {
        match self.root() {
            Some(root) => root,
            None => {
                let new_shared = Owned::new(Node::new()).into_shared(&self.guard);
                let result = self.root.compare_exchange(
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
