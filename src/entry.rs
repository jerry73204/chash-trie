use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};

use crate::error::Error;
use crate::node::Node;
use crate::GuardedTrie;

#[derive(Debug, Clone)]
pub struct Entry<'g, S, V, H> {
    pub(crate) node: &'g Node<S, V, H>,
    pub(crate) trie: &'g GuardedTrie<'g, S, V, H>,
}

impl<'g, S, V, H> Entry<'g, S, V, H>
where
    S: Eq + Hash,
    H: BuildHasher + Clone,
{
    pub fn get(&self) -> Option<&'g V> {
        self.node.get(self.trie)
    }

    pub fn try_insert(&self, value: V) -> Result<&'g V, Error> {
        self.node.insert(value, self.trie)
    }

    pub fn child<'a, Q, K>(&self, seg: &Q) -> Option<Entry<'g, S, V, H>>
    where
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let child_node = self.node.child(seg, self.trie)?;
        Some(Entry {
            node: child_node,
            trie: self.trie,
        })
    }

    pub fn find<'a, Q, K>(&'g self, key: K) -> Option<Entry<'g, S, V, H>>
    where
        K: IntoIterator<Item = &'a Q>,
        S: Borrow<Q>,
        Q: Hash + Eq + 'a,
    {
        let node = self.node.find(key, self.trie)?;
        Some(Entry {
            node,
            trie: self.trie,
        })
    }

    pub fn is_removed(&self) -> bool {
        self.node.is_removed()
    }
}
