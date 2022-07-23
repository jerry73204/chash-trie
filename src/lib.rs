mod node;
mod utils;

use crate::node::Node;
use crossbeam::epoch::{self, Guard};
use sharded_slab::Slab;
use std::borrow::Borrow;
use std::hash::Hash;
use std::sync::Arc;

#[derive(Debug)]
pub struct Trie<S, V>
where
    S: Eq + Hash,
{
    root_id: NodeId,
    slab: Arc<Slab<Node<S, V>>>,
}

impl<S, V> Trie<S, V>
where
    S: Eq + Hash,
{
    pub fn new() -> Self {
        let slab = Arc::new(Slab::new());
        let root_id = NodeId(slab.insert(Node::new()).unwrap());

        Self { root_id, slab }
    }

    pub fn pin(&self) -> GuardedTrie<'_, S, V> {
        GuardedTrie {
            guard: epoch::pin(),
            root_id: self.root_id,
            slab: &self.slab,
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
    root_id: NodeId,
    slab: &'g Arc<Slab<Node<S, V>>>,
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
        self.root().get(key, self)
    }

    pub fn insert<K>(&self, key: K, value: V) -> Option<&V>
    where
        K: IntoIterator<Item = S>,
    {
        self.root().insert(key, value, self)
    }

    pub fn remove<'a, Q, K>(&self, key: K) -> Option<&V>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let (value, _) = self.root().remove(key, self)?;
        Some(value)
    }

    // pub fn iter(&'g self) -> Box<dyn Iterator<Item = &'g V> + 'g> {
    //     self.root().iter(self)
    // }

    fn root(&self) -> sharded_slab::Entry<'_, Node<S, V>> {
        self.slab.get(self.root_id.0).unwrap()
    }

    // fn root_owned(&self) -> sharded_slab::OwnedEntry<Node<S, V>> {
    //     self.slab.clone().get_owned(self.root_id.0).unwrap()
    // }

    fn get_node(&self, id: NodeId) -> Option<sharded_slab::Entry<'_, Node<S, V>>> {
        self.slab.get(id.0)
    }

    fn create_node(&self) -> NodeId {
        let id = self.slab.insert(Node::new()).unwrap();
        NodeId(id)
    }

    fn remove_node(&self, id: NodeId) -> bool {
        self.slab.remove(id.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NodeId(pub usize);

// #[derive(Debug)]
// pub struct Entry<'g, S, V>
// where
//     S: Eq + Hash,
// {
//     entry: sharded_slab::Entry<'g, Node<S, V>>,
// }

// impl<'g, S, V> Entry<'g, S, V>
// where
//     S: Eq + Hash,
// {
//     pub fn value(&self) -> &V {
//         self.entry
//     }
// }
