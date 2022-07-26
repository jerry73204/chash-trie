use crate::{error::Error, GuardedTrie};
use crossbeam::epoch::{Atomic, Guard, Owned, Shared};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::borrow::Borrow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash};
use std::iter;
use std::sync::atomic::Ordering::*;
use std::sync::RwLock;
use std::thread::available_parallelism;

type ChildMap<S, V, H = RandomState> = DashMap<S, Atomic<Node<S, V, H>>, H>;

#[derive(Debug)]
pub(crate) struct Node<S, V, H> {
    pub(crate) children: Atomic<ChildMap<S, V, H>>,
    pub(crate) value: Atomic<V>,
    pub(crate) is_deleted: RwLock<bool>,
}

impl<S, V, H> Node<S, V, H>
where
    S: Eq + Hash,
    H: BuildHasher + Clone,
{
    pub fn new() -> Self {
        Self {
            children: Atomic::null(),
            value: Atomic::null(),
            is_deleted: RwLock::new(false),
        }
    }

    pub fn get_at<'a, 'g, Q, K>(&self, key: K, trie: &'g GuardedTrie<'g, S, V, H>) -> Option<&'g V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let mut key = key.into_iter();
        let guard = &trie.guard;

        // Get the value
        let value = match key.next() {
            Some(seg) => {
                let child_node = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        return None;
                    }

                    let entry = self.children(guard)?.get(seg)?;
                    let atomic = entry.value();
                    load_atomic(atomic, guard)?
                };
                child_node.get_at(key, trie)?
            }
            None => self.get(trie)?,
        };

        Some(value)
    }

    pub fn get<'g>(&self, trie: &'g GuardedTrie<'g, S, V, H>) -> Option<&'g V> {
        let guard = &trie.guard;

        let is_deleted = self.is_deleted.read().unwrap();
        if *is_deleted {
            return None;
        }

        self.value(guard)
    }

    pub fn child<'a, 'g, Q>(
        &self,
        seg: &Q,
        trie: &'g GuardedTrie<'g, S, V, H>,
    ) -> Option<&'g Node<S, V, H>>
    where
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let guard = &trie.guard;

        let is_deleted = self.is_deleted.read().unwrap();
        if *is_deleted {
            return None;
        }

        let entry = self.children(guard)?.get(seg)?;
        let atomic = entry.value();
        let child_node = load_atomic(atomic, guard)?;

        Some(child_node)
    }

    pub fn find<'a, 'g, Q, K>(
        &'g self,
        key: K,
        trie: &'g GuardedTrie<'g, S, V, H>,
    ) -> Option<&'g Node<S, V, H>>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let mut key = key.into_iter();
        let guard = &trie.guard;

        // Get the value
        let node = match key.next() {
            Some(seg) => {
                let child_node = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        return None;
                    }

                    let entry = self.children(guard)?.get(seg)?;
                    let atomic = entry.value();
                    load_atomic(atomic, guard)?
                };
                child_node.find(key, trie)?
            }
            None => {
                let is_deleted = self.is_deleted.read().unwrap();
                if *is_deleted {
                    return None;
                }

                self
            }
        };

        Some(node)
    }

    pub fn insert_at<'g, K>(
        &self,
        key: K,
        value: V,
        trie: &'g GuardedTrie<'g, S, V, H>,
    ) -> Result<&'g V, Error>
    where
        K: IntoIterator<Item = S>,
    {
        let mut key = key.into_iter();
        let guard = &trie.guard;

        match key.next() {
            Some(seg) => {
                let child_node = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        return Err(Error::Retry);
                    }
                    let entry = self
                        .get_or_create_children(trie)
                        .entry(seg)
                        .or_insert_with(|| Atomic::new(Node::new()));
                    let atomic = entry.value();
                    load_atomic(atomic, guard).ok_or(Error::NotFound)?
                };

                child_node.insert_at(key, value, trie)
            }
            None => self.insert(value, trie),
        }
    }

    pub fn insert<'g>(&self, value: V, trie: &'g GuardedTrie<'g, S, V, H>) -> Result<&'g V, Error> {
        let guard = &trie.guard;
        let is_deleted = self.is_deleted.read().unwrap();
        if *is_deleted {
            return Err(Error::Retry);
        }
        self.set_value(value, guard).ok_or(Error::NotFound)
    }

    pub fn remove_at<'a, 'g, Q, K>(
        &self,
        key: K,
        trie: &'g GuardedTrie<'g, S, V, H>,
    ) -> Result<(&'g V, bool), Error>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let mut key = key.into_iter();
        let guard = &trie.guard;

        // Get the value
        let (value, is_self_deleted) = match key.next() {
            Some(seg) => {
                // Find the related child
                let child_shared = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        return Err(Error::Retry);
                    }

                    let entry = self
                        .children(guard)
                        .ok_or(Error::NotFound)?
                        .get(seg)
                        .ok_or(Error::NotFound)?;
                    let atomic = entry.value();
                    atomic.load_consume(guard)
                };
                let child_node = unsafe { child_shared.as_ref().ok_or(Error::NotFound)? };

                // Delete the value in descendents. During the
                // process, the hash map entry for the child may be
                // set to null.
                let (value, is_child_deleted) = child_node.remove_at(key, trie)?;

                let is_self_deleted = {
                    let mut is_deleted = self.is_deleted.write().unwrap();

                    // Check if some deleter else removes this node already.
                    if *is_deleted {
                        return Ok((value, false));
                    }

                    let is_self_deleted = match self.children(guard) {
                        Some(children) => {
                            // If the child was deleted, try to remove the
                            // corresponding entry if the entry was not
                            // altered.
                            if is_child_deleted {
                                children.remove_if(seg, |_, atomic| {
                                    let result = atomic.compare_exchange(
                                        child_shared,
                                        Shared::null(),
                                        AcqRel,
                                        Release,
                                        guard,
                                    );
                                    result.is_ok()
                                });
                            }

                            children.is_empty() && self.value.load_consume(guard).is_null()
                        }
                        None => self.value.load_consume(guard).is_null(),
                    };

                    if is_self_deleted {
                        *is_deleted = true;
                    }

                    is_self_deleted
                };

                (value, is_self_deleted)
            }
            None => self.remove(trie)?,
        };

        Ok((value, is_self_deleted))
    }

    pub fn remove<'g>(&self, trie: &'g GuardedTrie<'g, S, V, H>) -> Result<(&'g V, bool), Error> {
        let guard = &trie.guard;
        let mut is_deleted = self.is_deleted.write().unwrap();

        // Check if some deleter else removes this node already.
        if *is_deleted {
            return Err(Error::NotFound);
        }

        // Get and unset the value.
        let value = self.take_value(guard).ok_or(Error::NotFound)?;

        // If this node has no children, ,mark this node
        // deleted and set the entry on parent to this node to
        // null.
        let is_self_deleted = match self.children(guard) {
            Some(children) => children.is_empty(),
            None => true,
        };

        if is_self_deleted {
            *is_deleted = true;
        }

        Ok((value, is_self_deleted))
    }

    pub fn iter<'g>(
        &'g self,
        trie: &'g GuardedTrie<'g, S, V, H>,
    ) -> Box<dyn Iterator<Item = &'g V> + 'g> {
        let guard = &trie.guard;
        let curr_value = iter::once_with(|| self.value(guard)).flatten();

        let child_values = self
            .children(guard)
            .into_iter()
            .flatten()
            .flat_map(|entry| {
                let child = entry.value();
                let shared = child.load_consume(guard);
                let ref_ = unsafe { shared.deref() };
                ref_.iter(trie)
            });

        let chain = curr_value.into_iter().chain(child_values);
        Box::new(chain)
    }

    pub fn is_removed(&self) -> bool {
        *self.is_deleted.read().unwrap()
    }

    fn value<'g>(&self, guard: &'g Guard) -> Option<&'g V> {
        let shared = self.value.load_consume(guard);
        unsafe { shared.as_ref() }
    }

    fn take_value<'g>(&self, guard: &'g Guard) -> Option<&'g V> {
        let shared = self.value.swap(Shared::null(), AcqRel, guard);
        unsafe { shared.as_ref() }
    }

    fn set_value<'g>(&self, new_value: V, guard: &'g Guard) -> Option<&'g V> {
        let new_value = Owned::new(new_value);
        let orig_shared = self.value.swap(new_value, AcqRel, guard);
        unsafe { orig_shared.as_ref() }
    }

    fn children<'g>(&self, guard: &'g Guard) -> Option<&'g ChildMap<S, V, H>> {
        let shared = self.children.load_consume(guard);
        unsafe { shared.as_ref() }
    }

    fn get_or_create_children<'g>(
        &self,
        trie: &'g GuardedTrie<'g, S, V, H>,
    ) -> &'g ChildMap<S, V, H> {
        let guard = &trie.guard;

        match self.children(guard) {
            Some(children) => children,
            None => {
                let map = Owned::new(new_map(&trie.trie.hash_builder));
                let result =
                    self.children
                        .compare_exchange(Shared::null(), map, AcqRel, Acquire, guard);
                let shared = match result {
                    Ok(curr) => curr,
                    Err(error) => error.current,
                };
                unsafe { shared.deref() }
            }
        }
    }
}

impl<S, V, H> Default for Node<S, V, H>
where
    S: Eq + Hash,
    H: BuildHasher + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

fn load_atomic<'g, T>(atomic: &Atomic<T>, guard: &'g Guard) -> Option<&'g T> {
    unsafe { atomic.load_consume(guard).as_ref() }
}

fn new_map<K, V, H>(build_hasher: &H) -> DashMap<K, V, H>
where
    K: Hash + Eq,
    H: BuildHasher + Clone,
{
    static DEFAULT_SHARD_AMOUNT: Lazy<usize> =
        Lazy::new(|| (available_parallelism().map_or(1, usize::from) * 4).next_power_of_two());

    DashMap::with_capacity_and_hasher_and_shard_amount(
        0,
        build_hasher.clone(),
        *DEFAULT_SHARD_AMOUNT,
    )
}
