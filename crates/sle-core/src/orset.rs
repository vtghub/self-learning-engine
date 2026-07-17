use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::id::Tag;

/// A generic add-wins Observed-Remove Set. An element is "present" if it has
/// at least one add-tag that isn't covered by a remove-tag. Removes only
/// tombstone the specific add-tags the remover had actually observed, so a
/// concurrent add (a tag the remover never saw) always survives a remove —
/// that's the "add-wins" property. Merge is a plain union of both maps,
/// which is trivially commutative, associative, and idempotent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrSet<E: Ord + Clone> {
    adds: BTreeMap<E, BTreeSet<Tag>>,
    removes: BTreeMap<E, BTreeSet<Tag>>,
}

impl<E: Ord + Clone> Default for OrSet<E> {
    fn default() -> Self {
        Self { adds: BTreeMap::new(), removes: BTreeMap::new() }
    }
}

impl<E: Ord + Clone> OrSet<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, elem: E, tag: Tag) {
        self.adds.entry(elem).or_default().insert(tag);
    }

    /// Tombstones every add-tag currently visible for `elem`. Any add-tag
    /// applied concurrently (not yet merged in) is untouched and will keep
    /// the element present once merged, per add-wins semantics.
    pub fn remove(&mut self, elem: &E) {
        if let Some(tags) = self.adds.get(elem).cloned() {
            self.removes.entry(elem.clone()).or_default().extend(tags);
        }
    }

    pub fn contains(&self, elem: &E) -> bool {
        match self.adds.get(elem) {
            None => false,
            Some(adds) => match self.removes.get(elem) {
                None => !adds.is_empty(),
                Some(removes) => adds.difference(removes).next().is_some(),
            },
        }
    }

    pub fn merge(&mut self, other: &Self) {
        for (elem, tags) in &other.adds {
            self.adds.entry(elem.clone()).or_default().extend(tags.iter().copied());
        }
        for (elem, tags) in &other.removes {
            self.removes.entry(elem.clone()).or_default().extend(tags.iter().copied());
        }
    }

    pub fn iter_present(&self) -> impl Iterator<Item = &E> {
        self.adds.keys().filter(|e| self.contains(e))
    }

    pub fn len_present(&self) -> usize {
        self.iter_present().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlc::Hlc;

    fn tag(replica: u64, physical: u64) -> Tag {
        Tag { replica, hlc: Hlc { physical, logical: 0 } }
    }

    #[test]
    fn add_then_present() {
        let mut s: OrSet<&str> = OrSet::new();
        s.add("a", tag(1, 1));
        assert!(s.contains(&"a"));
    }

    #[test]
    fn remove_removes() {
        let mut s: OrSet<&str> = OrSet::new();
        s.add("a", tag(1, 1));
        s.remove(&"a");
        assert!(!s.contains(&"a"));
    }

    #[test]
    fn concurrent_add_wins_over_remove() {
        // Replica A adds "a", replica B (independently, without seeing A's
        // add) can't remove a tag it never observed. Simulate by merging a
        // fresh concurrent add after a remove was already applied elsewhere.
        let mut a: OrSet<&str> = OrSet::new();
        a.add("a", tag(1, 1));
        a.remove(&"a");
        assert!(!a.contains(&"a"));

        let mut b: OrSet<&str> = OrSet::new();
        b.add("a", tag(2, 1)); // concurrent add, different tag, unseen by a's remove

        a.merge(&b);
        assert!(a.contains(&"a"), "concurrent add must win over an unrelated remove");
    }

    #[test]
    fn merge_is_commutative() {
        let mut a: OrSet<&str> = OrSet::new();
        a.add("x", tag(1, 1));
        let mut b: OrSet<&str> = OrSet::new();
        b.add("y", tag(2, 1));
        b.remove(&"y");

        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);

        assert_eq!(ab.contains(&"x"), ba.contains(&"x"));
        assert_eq!(ab.contains(&"y"), ba.contains(&"y"));
    }
}
