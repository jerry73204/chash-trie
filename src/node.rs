use crossbeam::epoch::{Atomic, Guard, Owned, Shared};
use std::borrow::Borrow;
use std::hash::Hash;
use std::iter;
use std::sync::atomic::Ordering::*;
use std::sync::RwLock;

use crate::{
    utils::{new_map, Map},
    GuardedTrie, NodeId,
};

#[derive(Debug)]
pub(crate) struct Node<S, V>
where
    S: Eq + Hash,
{
    pub(crate) children: Map<S, NodeId>,
    pub(crate) value: Atomic<V>,
    pub(crate) is_deleted: RwLock<bool>,
}

impl<S, V> Node<S, V>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            children: new_map(),
            value: Atomic::null(),
            is_deleted: RwLock::new(false),
        }
    }

    pub fn get<'a, 'g, Q, K>(&self, key: K, trie: &'g GuardedTrie<S, V>) -> Option<&'g V>
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

                    let child_id = *self.children.get(seg)?.value();
                    trie.get_node(child_id).unwrap()
                };
                child_node.get(key, trie)?
            }
            None => {
                let is_deleted = self.is_deleted.read().unwrap();
                if *is_deleted {
                    return None;
                }

                load_atomic(&self.value, guard)?
            }
        };

        Some(value)
    }

    pub fn insert<'g, K>(&self, key: K, value: V, trie: &'g GuardedTrie<S, V>) -> Option<&'g V>
    where
        K: IntoIterator<Item = S>,
    {
        let mut key = key.into_iter();
        let guard = &trie.guard;

        match key.next() {
            Some(seg) => {
                let ref_ = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        todo!("retry");
                    }
                    let child_id = *self
                        .children
                        .entry(seg)
                        .or_insert_with(|| trie.create_node());
                    trie.get_node(child_id).unwrap()
                };
                ref_.insert(key, value, trie)
            }
            None => {
                let is_deleted = self.is_deleted.read().unwrap();
                if *is_deleted {
                    todo!("retry");
                }
                self.set_value(value, guard)
            }
        }
    }

    pub fn remove<'a, 'g, Q, K>(&self, key: K, trie: &'g GuardedTrie<S, V>) -> Option<(&'g V, bool)>
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
                let (child_id, child_entry) = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        todo!("retry");
                    }

                    let child_id = *self.children.get(seg)?;
                    let child_entry = trie.get_node(child_id).unwrap();
                    (child_id, child_entry)
                };

                // Delete the value in descendents.
                let (value, is_child_deleted) = child_entry.remove(key, trie)?;
                drop(child_entry);

                let is_self_deleted = {
                    let mut is_deleted = self.is_deleted.write().unwrap();

                    // Check if some deleter else removes this node already.
                    if *is_deleted {
                        return Some((value, false));
                    }

                    // If the child was deleted in this thread, remove
                    // the corresponding entry.
                    if is_child_deleted {
                        self.children.remove(seg);
                        let ok = trie.remove_node(child_id);
                        assert!(ok);
                    }

                    // If the node has no children and the value is
                    // unset, mark his node deleted and delete the
                    // entry on parent to this node.
                    let is_self_deleted =
                        self.children.is_empty() && self.value.load_consume(guard).is_null();

                    if is_self_deleted {
                        *is_deleted = true;
                    }

                    is_self_deleted
                };

                (value, is_self_deleted)
            }
            None => {
                let mut is_deleted = self.is_deleted.write().unwrap();

                // Check if some deleter else removes this node already.
                if *is_deleted {
                    return None;
                }

                // Get and unset the value.
                let value = load_atomic(&self.value, guard)?;
                self.value.store(Shared::null(), Release);

                // If this node has no children, ,mark this node
                // deleted and set the entry on parent to this node to
                // null.
                let is_self_deleted = self.children.is_empty();
                if is_self_deleted {
                    *is_deleted = true;
                }

                (value, is_self_deleted)
            }
        };

        Some((value, is_self_deleted))
    }

    // pub fn iter<'g>(&self, trie: &'g GuardedTrie<S, V>) -> Box<dyn Iterator<Item = &'g V> + 'g> {
    //     let guard = &trie.guard;
    //     let curr_value = iter::once_with(|| load_atomic(&self.value, guard)).flatten();

    //     let child_values = self.children.iter().flat_map(|entry| {
    //         let child = entry.value();
    //         let shared = child.load_consume(guard);
    //         let ref_ = unsafe { shared.deref() };
    //         ref_.iter(trie)
    //     });

    //     let chain = curr_value.into_iter().chain(child_values);
    //     Box::new(chain)
    // }

    // fn value<'g>(&'g self, guard: &'g Guard) -> Option<&'g V> {
    //     let shared = self.value.load_consume(guard);
    //     unsafe { shared.as_ref() }
    // }

    fn set_value<'g>(&self, new_value: V, guard: &'g Guard) -> Option<&'g V> {
        let new_value = Owned::new(new_value);
        let orig_shared = self.value.swap(new_value, AcqRel, guard);
        unsafe { orig_shared.as_ref() }
    }
}

impl<S, V> Default for Node<S, V>
where
    S: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

fn load_atomic<'g, T>(atomic: &Atomic<T>, guard: &'g Guard) -> Option<&'g T> {
    unsafe { atomic.load_consume(guard).as_ref() }
}
