use crossbeam::epoch::{Atomic, Guard, Owned, Shared};
use dashmap::DashMap;
use std::borrow::Borrow;
use std::hash::Hash;
use std::iter;
use std::sync::atomic::Ordering::*;
use std::sync::RwLock;

#[derive(Debug)]
pub(crate) struct Node<S, V>
where
    S: Eq + Hash,
{
    pub(crate) children: DashMap<S, Atomic<Node<S, V>>>,
    pub(crate) value: Atomic<V>,
    pub(crate) is_deleted: RwLock<bool>,
}

impl<S, V> Node<S, V>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            children: DashMap::new(),
            value: Atomic::null(),
            is_deleted: RwLock::new(false),
        }
    }

    pub fn get<'a, 'g, Q, K>(&'g self, key: K, guard: &'g Guard) -> Option<&'g V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let mut key = key.into_iter();

        // Get the value
        let value = match key.next() {
            Some(seg) => {
                let child_node = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        return None;
                    }

                    let entry = self.children.get(seg)?;
                    let atomic = entry.value();
                    load_atomic(atomic, guard)?
                };
                child_node.get(key, guard)?
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

    pub fn insert<'g, K>(&'g self, key: K, value: V, guard: &'g Guard) -> Option<&'g V>
    where
        K: IntoIterator<Item = S>,
    {
        let mut key = key.into_iter();

        match key.next() {
            Some(seg) => {
                let ref_ = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        todo!("retry");
                    }
                    let entry = self
                        .children
                        .entry(seg)
                        .or_insert_with(|| Atomic::new(Node::new()));
                    let atomic = entry.value();
                    load_atomic(atomic, guard)?
                };
                ref_.insert(key, value, guard)
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

    pub fn remove<'a, 'g, Q, K>(
        &'g self,
        key: K,
        parent: Option<(&Self, &Q, Shared<'g, Self>)>,
        guard: &'g Guard,
    ) -> Option<&'g V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let mut key = key.into_iter();

        // Get the value
        let value = match key.next() {
            Some(seg) => {
                // Find the related child
                let shared = {
                    let is_deleted = self.is_deleted.read().unwrap();
                    if *is_deleted {
                        todo!("retry");
                    }

                    let entry = self.children.get(seg)?;
                    let atomic = entry.value();
                    atomic.load_consume(guard)
                };
                let child_node = unsafe { shared.as_ref() }?;

                // Delete the value in descendents. During the
                // process, the hash map entry for the child may be
                // set to null.
                let value = child_node.remove(key, Some((self, seg, shared)), guard)?;

                {
                    let mut is_deleted = self.is_deleted.write().unwrap();

                    // Check if some deleter else removes this node already.
                    if *is_deleted {
                        return Some(value);
                    }

                    // If the entry is set to null, remove it.
                    if let Some(entry) = self.children.get(seg) {
                        if load_atomic(entry.value(), guard).is_none() {
                            self.children.remove(seg);
                        }
                    }

                    // If the node has no children and the value is
                    // unset, mark his node deleted and delete the
                    // entry on parent to this node.
                    if self.children.is_empty() && self.value.load_consume(guard).is_null() {
                        *is_deleted = true;

                        if let Some((parent, prev_seg, expect_value)) = parent {
                            if let Some(atomic) = parent.children.get(prev_seg) {
                                let _ = atomic.compare_exchange(
                                    expect_value,
                                    Shared::null(),
                                    AcqRel,
                                    Acquire,
                                    guard,
                                );
                            }
                        }
                    }
                }

                value
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
                if self.children.is_empty() {
                    *is_deleted = true;

                    if let Some((parent, prev_seg, expect_value)) = parent {
                        if let Some(atomic) = parent.children.get(prev_seg) {
                            let _ = atomic.compare_exchange(
                                expect_value,
                                Shared::null(),
                                AcqRel,
                                Acquire,
                                guard,
                            );
                        }
                    }
                }
                value
            }
        };

        Some(value)
    }

    pub fn iter<'g>(&'g self, guard: &'g Guard) -> Box<dyn Iterator<Item = &'g V> + 'g> {
        let curr_value = iter::once_with(|| load_atomic(&self.value, guard)).flatten();

        let child_values = self.children.iter().flat_map(|entry| {
            let child = entry.value();
            let shared = child.load_consume(guard);
            let ref_ = unsafe { shared.deref() };
            ref_.iter(guard)
        });

        let chain = curr_value.into_iter().chain(child_values);
        Box::new(chain)
    }

    // fn value<'g>(&'g self, guard: &'g Guard) -> Option<&'g V> {
    //     let shared = self.value.load_consume(guard);
    //     unsafe { shared.as_ref() }
    // }

    fn set_value<'g>(&'g self, new_value: V, guard: &'g Guard) -> Option<&'g V> {
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
