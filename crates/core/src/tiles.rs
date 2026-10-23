//! Indexing of the tiles of a dimension.
//!
//! The app's own renderer writes `zoom.<z>/t.<x>.<y>.png`. The parser is deliberately more
//! tolerant (`tile.X.Y.png`, `X_Y.png`, a `zoom0` folder, ...) so that snapshots made with
//! older versions (uNmINeD tiles) are still listed. The manifest stores each tile's real
//! relative path, so the viewer never has to guess it.

use crate::{
    error::Result,
    hashing,
    model::{DimTiles, TileRow},
};
use rayon::prelude::*;
use regex::Regex;
use std::path::Path;
use walkdir::WalkDir;

pub fn parse_tile_path(rel: &str) -> Option<(i32, i32, i32)> {
    let file_re = Regex::new(r"(?i)(?:^|[^\d-])(-?\d+)[._](-?\d+)\.(?:png|jpe?g|webp)$").unwrap();
    let zoom_re = Regex::new(r"(?i)zoom[._ -]?(-?\d+)").unwrap();

    let file = rel.rsplit('/').next()?;
    let c = file_re.captures(file)?;
    let x: i32 = c[1].parse().ok()?;
    let y: i32 = c[2].parse().ok()?;

    let dirs = &rel[..rel.len() - file.len()];
    let z = zoom_re
        .captures(dirs)
        .and_then(|zc| zc[1].parse::<i32>().ok())
        .unwrap_or(0);
    Some((z, x, y))
}

/// Walk a dimension's render folder and hash every tile.
pub fn index_tiles(dim_dir: &Path) -> Result<DimTiles> {
    let root = if dim_dir.join("tiles").is_dir() { dim_dir.join("tiles") } else { dim_dir.to_path_buf() };
    let base = dim_dir;

    let candidates: Vec<(i32, i32, i32, String, std::path::PathBuf)> = WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(base).ok()?.to_string_lossy().replace('\\', "/");
            let (z, x, y) = parse_tile_path(&rel)?;
            Some((z, x, y, rel, e.path().to_path_buf()))
        })
        .collect();

    let rows: Result<Vec<TileRow>> = candidates
        .par_iter()
        .map(|(z, x, y, rel, abs)| Ok((*z, *x, *y, rel.clone(), hashing::hash_file(abs)?)))
        .collect();
    let mut tiles = rows?;
    tiles.sort_by(|a, b| (a.0, a.2, a.1).cmp(&(b.0, b.2, b.1)));
    Ok(DimTiles { tiles })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_layouts() {
        assert_eq!(parse_tile_path("zoom.-2/t.3.-4.png"), Some((-2, 3, -4)));
        assert_eq!(parse_tile_path("zoom.0/t.-1.0.png"), Some((0, -1, 0)));
        assert_eq!(parse_tile_path("zoom.0/t.1.2.png.tmp"), None);
        assert_eq!(parse_tile_path("sources.json"), None);
        assert_eq!(parse_tile_path("tiles/zoom.0/0/tile.3.-2.png"), Some((0, 3, -2)));
        assert_eq!(parse_tile_path("tiles/zoom.-1/0/tile.0.0.png"), Some((-1, 0, 0)));
        assert_eq!(parse_tile_path("tiles/zoom2/5_6.png"), Some((2, 5, 6)));
        assert_eq!(parse_tile_path("tiles/7.8.png"), Some((0, 7, 8)));
        assert_eq!(parse_tile_path("unmined.index.html"), None);
        assert_eq!(parse_tile_path("tiles/zoom.0/0/preview.png"), None);
    }
}
