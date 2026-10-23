use crate::model::{DiffFile, DimDiff, Dimension, TilesManifest};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Compare two snapshots tile by tile at the most detailed zoom level of each dimension.
/// Dimensions missing in the previous snapshot, or rendered at a different detail level,
/// are skipped: there is nothing meaningful to compare.
pub fn compute(prev: &TilesManifest, cur: &TilesManifest, previous_id: Option<String>) -> DiffFile {
    let mut out = DiffFile { previous: previous_id, dimensions: BTreeMap::new() };
    // Tiles of different renderers or sizes cannot be compared.
    if prev.tile_size != cur.tile_size || prev.renderer != cur.renderer {
        return out;
    }

    for (dim, cur_tiles) in &cur.dimensions {
        let Some(prev_tiles) = prev.dimensions.get(dim) else { continue };
        let Some(z) = cur_tiles.tiles.iter().map(|t| t.0).max() else { continue };

        let prev_at_z: HashMap<(i32, i32), &str> = prev_tiles
            .tiles
            .iter()
            .filter(|t| t.0 == z)
            .map(|t| ((t.1, t.2), t.4.as_str()))
            .collect();
        if prev_at_z.is_empty() {
            continue;
        }
        let cur_at_z: HashMap<(i32, i32), &str> = cur_tiles
            .tiles
            .iter()
            .filter(|t| t.0 == z)
            .map(|t| ((t.1, t.2), t.4.as_str()))
            .collect();

        let mut d = DimDiff { z, ..Default::default() };
        for (k, h) in &cur_at_z {
            match prev_at_z.get(k) {
                None => d.added.push(*k),
                Some(ph) if ph != h => d.changed.push(*k),
                _ => {}
            }
        }
        for k in prev_at_z.keys() {
            if !cur_at_z.contains_key(k) {
                d.removed.push(*k);
            }
        }
        d.added.sort();
        d.changed.sort();
        d.removed.sort();
        out.dimensions.insert(*dim, d);
    }
    out
}

/// Union of several diffs (the changes of a whole day), per dimension and kind.
/// Only diffs at the same zoom level as the first one seen for a dimension are merged.
pub fn union<'a>(diffs: impl IntoIterator<Item = &'a DiffFile>) -> BTreeMap<Dimension, DimDiff> {
    type Sets = (i32, BTreeSet<(i32, i32)>, BTreeSet<(i32, i32)>, BTreeSet<(i32, i32)>);
    let mut acc: BTreeMap<Dimension, Sets> = BTreeMap::new();
    for diff in diffs {
        for (dim, d) in &diff.dimensions {
            let e = acc
                .entry(*dim)
                .or_insert_with(|| (d.z, BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
            if e.0 != d.z {
                continue;
            }
            e.1.extend(d.added.iter().copied());
            e.2.extend(d.changed.iter().copied());
            e.3.extend(d.removed.iter().copied());
        }
    }
    acc.into_iter()
        .map(|(dim, (z, a, c, r))| {
            (
                dim,
                DimDiff {
                    z,
                    added: a.into_iter().collect(),
                    changed: c.into_iter().collect(),
                    removed: r.into_iter().collect(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DimTiles;

    fn manifest(rows: &[(i32, i32, i32, &str)]) -> TilesManifest {
        let mut m = TilesManifest { tile_size: 256, ..Default::default() };
        m.dimensions.insert(
            Dimension::Overworld,
            DimTiles {
                tiles: rows
                    .iter()
                    .map(|(z, x, y, h)| (*z, *x, *y, format!("t/{x}.{y}.png"), h.to_string()))
                    .collect(),
            },
        );
        m
    }

    #[test]
    fn detects_added_changed_removed() {
        let prev = manifest(&[(0, 0, 0, "a"), (0, 1, 0, "b"), (0, 2, 0, "c"), (-1, 0, 0, "z")]);
        let cur = manifest(&[(0, 0, 0, "a"), (0, 1, 0, "B"), (0, 3, 0, "d"), (-1, 0, 0, "Z")]);
        let d = compute(&prev, &cur, Some("p".into()));
        let o = &d.dimensions[&Dimension::Overworld];
        assert_eq!(o.z, 0);
        assert_eq!(o.added, vec![(3, 0)]);
        assert_eq!(o.changed, vec![(1, 0)]);
        assert_eq!(o.removed, vec![(2, 0)]);
    }

    #[test]
    fn different_renderers_are_not_compared() {
        let mut prev = manifest(&[(0, 0, 0, "a")]);
        let cur = manifest(&[(0, 0, 0, "b")]);
        prev.renderer = "old".into();
        assert!(compute(&prev, &cur, None).dimensions.is_empty());
    }

    #[test]
    fn skips_dimension_missing_before() {
        let prev = TilesManifest { tile_size: 256, ..Default::default() };
        let cur = manifest(&[(0, 0, 0, "a")]);
        assert!(compute(&prev, &cur, None).dimensions.is_empty());
    }
}
