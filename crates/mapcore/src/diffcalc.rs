//! Pure tile-hash comparison and merging of change sets.

use crate::tilepath::TileKey;
use std::collections::{BTreeMap, HashMap};

/// Kind of change for a tile between two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Modified,
    Removed,
}

impl Change {
    pub fn code(self) -> &'static str {
        match self {
            Change::Added => "a",
            Change::Modified => "m",
            Change::Removed => "r",
        }
    }
    pub fn from_code(s: &str) -> Option<Change> {
        match s {
            "a" => Some(Change::Added),
            "m" => Some(Change::Modified),
            "r" => Some(Change::Removed),
            _ => None,
        }
    }
}

/// Compare two tile-hash maps. Output is sorted by key for determinism.
pub fn diff_tiles(
    old: &HashMap<TileKey, String>,
    new: &HashMap<TileKey, String>,
) -> Vec<(TileKey, Change)> {
    let mut out: Vec<(TileKey, Change)> = Vec::new();
    for (k, h) in new {
        match old.get(k) {
            None => out.push((*k, Change::Added)),
            Some(oh) if oh != h => out.push((*k, Change::Modified)),
            _ => {}
        }
    }
    for k in old.keys() {
        if !new.contains_key(k) {
            out.push((*k, Change::Removed));
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out
}

/// Combine two consecutive changes of the same tile into the net change.
/// `None` means the changes cancel out (added, then removed again).
pub fn merge_change(first: Change, then: Change) -> Option<Change> {
    use Change::*;
    match (first, then) {
        (Added, Modified) => Some(Added),
        (Added, Removed) => None,
        (Modified, Removed) => Some(Removed),
        (Removed, Added) => Some(Modified),
        (_, t) => Some(t),
    }
}

/// Accumulates the net changes of consecutive diffs (used for the day slider,
/// which shows everything that happened during a day).
#[derive(Default, Debug)]
pub struct ChangeAccumulator {
    map: BTreeMap<TileKey, Change>,
}

impl ChangeAccumulator {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn apply(&mut self, changes: &[(TileKey, Change)]) {
        for (k, c) in changes {
            match self.map.get(k).copied() {
                None => {
                    self.map.insert(*k, *c);
                }
                Some(prev) => match merge_change(prev, *c) {
                    Some(net) => {
                        self.map.insert(*k, net);
                    }
                    None => {
                        self.map.remove(k);
                    }
                },
            }
        }
    }
    pub fn into_vec(self) -> Vec<(TileKey, Change)> {
        self.map.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(items: &[(TileKey, &str)]) -> HashMap<TileKey, String> {
        items.iter().map(|(k, h)| (*k, h.to_string())).collect()
    }

    #[test]
    fn detects_added_modified_removed() {
        let old = m(&[((0, 0, 0), "a"), ((0, 1, 0), "b"), ((0, 2, 0), "c")]);
        let new = m(&[((0, 0, 0), "a"), ((0, 1, 0), "B"), ((0, 3, 0), "d")]);
        let d = diff_tiles(&old, &new);
        assert_eq!(
            d,
            vec![
                ((0, 1, 0), Change::Modified),
                ((0, 2, 0), Change::Removed),
                ((0, 3, 0), Change::Added),
            ]
        );
    }

    #[test]
    fn identical_is_empty() {
        let a = m(&[((0, 0, 0), "a")]);
        assert!(diff_tiles(&a, &a.clone()).is_empty());
    }

    #[test]
    fn merge_rules() {
        use Change::*;
        assert_eq!(merge_change(Added, Modified), Some(Added));
        assert_eq!(merge_change(Added, Removed), None);
        assert_eq!(merge_change(Modified, Removed), Some(Removed));
        assert_eq!(merge_change(Removed, Added), Some(Modified));
        assert_eq!(merge_change(Modified, Modified), Some(Modified));
    }

    #[test]
    fn accumulator_nets_out() {
        let mut acc = ChangeAccumulator::new();
        acc.apply(&[((0, 0, 0), Change::Added), ((0, 1, 1), Change::Modified)]);
        acc.apply(&[
            ((0, 0, 0), Change::Removed),
            ((0, 1, 1), Change::Modified),
            ((0, 2, 2), Change::Added),
        ]);
        assert_eq!(
            acc.into_vec(),
            vec![((0, 1, 1), Change::Modified), ((0, 2, 2), Change::Added)]
        );
    }

    #[test]
    fn codes_roundtrip() {
        for c in [Change::Added, Change::Modified, Change::Removed] {
            assert_eq!(Change::from_code(c.code()), Some(c));
        }
        assert_eq!(Change::from_code("x"), None);
    }
}
