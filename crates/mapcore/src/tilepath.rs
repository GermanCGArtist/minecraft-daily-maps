//! Recognises tile coordinates from file paths produced by the renderer.
//!
//! uNmINeD's exact on-disk tile layout is not something we want to hard-wire,
//! so this module accepts the plausible shapes and normalises them to
//! `(zoom, x, y)`:
//!
//!   zoom.-1/0/0/tile.3.-2.png      (zoom folder + tile.X.Y.png)
//!   zoom.0/tile.3.-2.png
//!   tiles/z2/tile.3.-2.png
//!   2/3/-2.png                     (our own normalised layout z/x/y.png)

pub type TileKey = (i32, i32, i32); // (zoom, x, y)

fn parse_zoom_component(c: &str) -> Option<i32> {
    let lower = c.to_ascii_lowercase();
    for prefix in ["zoom.", "zoom_", "zoom-", "zoom", "z"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.is_empty() {
                continue;
            }
            if let Ok(v) = rest.parse::<i32>() {
                return Some(v);
            }
        }
    }
    None
}

fn parse_tile_filename(name: &str) -> Option<(i32, i32)> {
    let lower = name.to_ascii_lowercase();
    let base = lower.strip_suffix(".png")?;
    let rest = base
        .strip_prefix("tile.")
        .or_else(|| base.strip_prefix("tile_"))
        .or_else(|| base.strip_prefix("tile-"))?;
    // "X.Y" or "X_Y" - coordinates may be negative, so split on the separator
    // that is not a leading '-'.
    for sep in ['.', '_'] {
        if let Some(idx) = rest.find(sep) {
            let (a, b) = rest.split_at(idx);
            let b = &b[1..];
            if let (Ok(x), Ok(y)) = (a.parse::<i32>(), b.parse::<i32>()) {
                return Some((x, y));
            }
        }
    }
    None
}

/// `rel` is a path relative to the render output directory, using either
/// `/` or `\` as separator.
pub fn parse_tile_path(rel: &str) -> Option<TileKey> {
    let comps: Vec<&str> = rel
        .split(|c| c == '/' || c == '\\')
        .filter(|s| !s.is_empty())
        .collect();
    let file = *comps.last()?;
    let dirs = &comps[..comps.len() - 1];

    // Shape 1: zoom folder somewhere + tile.X.Y.png
    if let Some((x, y)) = parse_tile_filename(file) {
        let z = dirs.iter().rev().find_map(|d| parse_zoom_component(d))?;
        return Some((z, x, y));
    }

    // Shape 2: <z>/<x>/<y>.png (plain integers)
    let stem = file
        .strip_suffix(".png")
        .or_else(|| file.strip_suffix(".PNG"))?;
    let y = stem.parse::<i32>().ok()?;
    if dirs.len() >= 2 {
        let x = dirs[dirs.len() - 1].parse::<i32>().ok()?;
        let z = dirs[dirs.len() - 2].parse::<i32>().ok()?;
        return Some((z, x, y));
    }
    None
}

/// Relative path of a tile in the normalised layout.
pub fn normalised_rel(key: TileKey) -> String {
    format!("{}/{}/{}.png", key.0, key.1, key.2)
}

/// Axis-aligned tile bounds per zoom level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileBounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

pub fn bounds_per_zoom<'a, I: IntoIterator<Item = &'a TileKey>>(keys: I) -> Vec<(i32, TileBounds)> {
    let mut out: Vec<(i32, TileBounds)> = Vec::new();
    for &(z, x, y) in keys {
        match out.iter_mut().find(|(zz, _)| *zz == z) {
            Some((_, b)) => {
                b.min_x = b.min_x.min(x);
                b.max_x = b.max_x.max(x);
                b.min_y = b.min_y.min(y);
                b.max_y = b.max_y.max(y);
            }
            None => out.push((
                z,
                TileBounds {
                    min_x: x,
                    max_x: x,
                    min_y: y,
                    max_y: y,
                },
            )),
        }
    }
    out.sort_by_key(|(z, _)| *z);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_folder_and_tile_name() {
        assert_eq!(
            parse_tile_path("zoom.-1/0/0/tile.3.-2.png"),
            Some((-1, 3, -2))
        );
        assert_eq!(
            parse_tile_path("tiles/zoom.0/tile.-10.4.png"),
            Some((0, -10, 4))
        );
        assert_eq!(
            parse_tile_path("tiles\\zoom.2\\tile.1.1.png"),
            Some((2, 1, 1))
        );
    }
    #[test]
    fn short_zoom_folder() {
        assert_eq!(parse_tile_path("z2/tile.3.-2.png"), Some((2, 3, -2)));
    }
    #[test]
    fn normalised_layout_roundtrip() {
        let k = (1, -4, 7);
        assert_eq!(parse_tile_path(&normalised_rel(k)), Some(k));
        assert_eq!(parse_tile_path("overworld/1/-4/7.png"), Some((1, -4, 7)));
    }
    #[test]
    fn rejects_non_tiles() {
        assert_eq!(parse_tile_path("favicon.png"), None);
        assert_eq!(parse_tile_path("images/icon.png"), None);
        assert_eq!(parse_tile_path("tile.1.2.png"), None); // no zoom info
        assert_eq!(parse_tile_path("zoom.0/readme.txt"), None);
    }
    #[test]
    fn bounds() {
        let keys = vec![(0, -1, 2), (0, 3, -4), (1, 5, 5)];
        let b = bounds_per_zoom(&keys);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].0, 0);
        assert_eq!(
            b[0].1,
            TileBounds {
                min_x: -1,
                max_x: 3,
                min_y: -4,
                max_y: 2
            }
        );
        assert_eq!(
            b[1].1,
            TileBounds {
                min_x: 5,
                max_x: 5,
                min_y: 5,
                max_y: 5
            }
        );
    }
}
