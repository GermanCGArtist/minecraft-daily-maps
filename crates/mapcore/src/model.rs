//! Data formats shared by the pipeline, the viewer and the desktop UI:
//! `config.json`, `render/meta.json`, `diff.json`, `days.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;
pub const TILE_SIZE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

impl Dimension {
    pub const ALL: [Dimension; 3] = [Dimension::Overworld, Dimension::Nether, Dimension::End];

    /// Folder / JSON key, also the value passed to `unmined-cli --dimension`.
    pub fn key(self) -> &'static str {
        match self {
            Dimension::Overworld => "overworld",
            Dimension::Nether => "nether",
            Dimension::End => "end",
        }
    }

    /// Region folder relative to the world root.
    pub fn region_subdir(self) -> &'static str {
        match self {
            Dimension::Overworld => "region",
            Dimension::Nether => "DIM-1/region",
            Dimension::End => "DIM1/region",
        }
    }

    pub fn from_key(s: &str) -> Option<Dimension> {
        match s {
            "overworld" => Some(Dimension::Overworld),
            "nether" => Some(Dimension::Nether),
            "end" => Some(Dimension::End),
            _ => None,
        }
    }
}

/// Path to the uNmINeD CLI per operating system.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UnminedPaths {
    pub macos: String,
    pub windows: String,
    pub linux: String,
}

impl UnminedPaths {
    pub fn current(&self) -> &str {
        if cfg!(target_os = "macos") {
            &self.macos
        } else if cfg!(target_os = "windows") {
            &self.windows
        } else {
            &self.linux
        }
    }
    pub fn set_current(&mut self, p: String) {
        if cfg!(target_os = "macos") {
            self.macos = p;
        } else if cfg!(target_os = "windows") {
            self.windows = p;
        } else {
            self.linux = p;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    pub unmined_paths: UnminedPaths,
    pub default_dimensions: Vec<Dimension>,
    /// `--zoomin` passed to uNmINeD (each level multiplies the tile count by 4).
    pub zoom_in: u8,
    /// Keep the extracted `world/` folder after rendering.
    pub keep_world: bool,
    /// Hard-link unchanged tiles from the previous snapshot instead of copying
    /// (only used when a dimension is reused without re-rendering).
    pub link_unchanged_tiles: bool,
    /// Import zips dropped into `inbox/` without waiting for confirmation.
    pub auto_import_inbox: bool,
    /// Extra arguments appended to the `unmined-cli web render` call.
    pub extra_unmined_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema: SCHEMA_VERSION,
            unmined_paths: UnminedPaths::default(),
            default_dimensions: vec![Dimension::Overworld],
            zoom_in: 1,
            keep_world: false,
            link_unchanged_tiles: false,
            auto_import_inbox: false,
            extra_unmined_args: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bounds {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DimensionMeta {
    pub tile_size: u32,
    pub zoom_min: i32,
    pub zoom_max: i32,
    /// Tile bounds per zoom level (key = zoom as string).
    pub bounds: BTreeMap<String, Bounds>,
    pub tile_count: usize,
    pub region_count: usize,
    /// True when no region file changed and the previous tiles were reused.
    pub reused_previous: bool,
    /// `--zoomin` value the tiles were rendered with.
    #[serde(default)]
    pub zoom_in: u8,
}

/// `render/meta.json`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Meta {
    pub schema: u32,
    pub date: String,           // YYYY-MM-DD (local)
    pub time: String,           // HHMM (local)
    pub local_datetime: String, // YYYY-MM-DDTHH:MM:00
    pub utc_offset: String,     // +02:00
    pub source_zip: String,
    pub zip_sha256: String,
    pub rendered_at: String,
    pub dimensions: BTreeMap<String, DimensionMeta>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotId {
    pub date: String,
    pub time: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DimensionDiff {
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    /// True when the previous snapshot had no tiles for this dimension, so
    /// nothing is reported as changed.
    pub baseline: bool,
    /// zoom (string) -> [[x, y, "a"|"m"|"r"], ...]
    pub tiles: BTreeMap<String, Vec<(i32, i32, String)>>,
}

/// `diff.json`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiffFile {
    pub schema: u32,
    pub against: Option<SnapshotId>,
    pub dimensions: BTreeMap<String, DimensionDiff>,
}

/// What the UI needs to know about one snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub date: String,
    pub time: String,
    pub local_datetime: String,
    pub utc_offset: String,
    pub dimensions: Vec<String>,
    pub has_world: bool,
    pub zip_name: Option<String>,
    pub tile_count: usize,
    pub changed_tiles: usize,
    pub complete: bool,
}
