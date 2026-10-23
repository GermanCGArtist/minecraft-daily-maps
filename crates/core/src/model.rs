//! Data formats: config.json, meta.json, tiles.json, diff.json.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

impl Dimension {
    pub const ALL: [Dimension; 3] = [Dimension::Overworld, Dimension::Nether, Dimension::End];

    pub fn as_str(self) -> &'static str {
        match self {
            Dimension::Overworld => "overworld",
            Dimension::Nether => "nether",
            Dimension::End => "end",
        }
    }

    /// Possible region folders inside a world folder (classic layout first, then the
    /// newer `dimensions/minecraft/...` layout).
    pub fn region_candidates(self) -> &'static [&'static str] {
        match self {
            Dimension::Overworld => &["region", "dimensions/minecraft/overworld/region"],
            Dimension::Nether => &["DIM-1/region", "dimensions/minecraft/the_nether/region"],
            Dimension::End => &["DIM1/region", "dimensions/minecraft/the_end/region"],
        }
    }
}

/// `config.json` inside the data folder. Unknown keys (e.g. the uNmINeD settings of older
/// versions) are ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_dimensions: Vec<Dimension>,
    /// Keep the extracted world after rendering (needed for fast re-renders).
    pub keep_world: bool,
    /// Also write one stitched PNG per dimension (shrunk to fit `stitched_max_px`).
    pub stitched_image: bool,
    /// Longest side of the stitched image in pixels.
    pub stitched_max_px: u32,
    /// Worker threads for drawing tiles. 0 = all cores but one.
    pub render_threads: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_dimensions: vec![Dimension::Overworld],
            keep_world: false,
            stitched_image: false,
            stitched_max_px: 16384,
            render_threads: 0,
        }
    }
}

/// `render/meta.json` of a snapshot. A snapshot only counts as complete once this exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// `YYYY-MM-DD`, local time.
    pub date: String,
    /// `HHMM`, local time.
    pub time: String,
    /// UTC offset of the local time, e.g. `+02:00`.
    pub utc_offset: String,
    pub source_name: String,
    /// BLAKE3 of the zip. Older snapshots stored a SHA-256 here under the name `zip_sha256`.
    #[serde(alias = "zip_sha256")]
    pub zip_hash: String,
    pub dimensions: Vec<Dimension>,
    /// Only used by snapshots made with uNmINeD (v0.1 - v0.3).
    #[serde(default)]
    pub zoom_in: u32,
    pub tile_size: u32,
    /// Which renderer drew the tiles. Empty for snapshots made with uNmINeD.
    #[serde(default)]
    pub renderer: String,
    pub world_kept: bool,
    pub created_at: String,
}

impl SnapshotMeta {
    /// `2026-10-22/0005`
    pub fn id(&self) -> String {
        format!("{}/{}", self.date, self.time)
    }
}

/// `incomplete.json` in a snapshot folder whose import did not finish (renderer crashed,
/// disk full, ...). The partial render is kept so the import can be resumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incomplete {
    pub date: String,
    pub time: String,
    pub source_name: String,
    pub zip_hash: String,
    pub dimensions: Vec<Dimension>,
    pub created_at: String,
    pub last_error: String,
}

/// One tile: zoom, x, y, path relative to the dimension's render folder, BLAKE3 hash.
pub type TileRow = (i32, i32, i32, String, String);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DimTiles {
    pub tiles: Vec<TileRow>,
}

/// `render/tiles.json`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TilesManifest {
    pub tile_size: u32,
    #[serde(default)]
    pub renderer: String,
    pub dimensions: BTreeMap<Dimension, DimTiles>,
}

/// Tile changes at the most detailed zoom level of a dimension.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DimDiff {
    pub z: i32,
    pub added: Vec<(i32, i32)>,
    pub changed: Vec<(i32, i32)>,
    pub removed: Vec<(i32, i32)>,
}

/// `diff.json` of a snapshot: changes compared with the previous snapshot in time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffFile {
    pub previous: Option<String>,
    pub dimensions: BTreeMap<Dimension, DimDiff>,
}
