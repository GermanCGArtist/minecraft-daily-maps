//! On-disk layout of the data folder and snapshot discovery.

use crate::model::*;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

/// Lightweight reference to a snapshot folder.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SnapRef {
    pub date: String,
    pub id: String, // HHMM
}

impl Store {
    pub fn new<P: Into<PathBuf>>(root: P) -> Store {
        Store { root: root.into() }
    }

    pub fn inbox_dir(&self) -> PathBuf {
        self.root.join("inbox")
    }
    pub fn days_dir(&self) -> PathBuf {
        self.root.join("days")
    }
    pub fn site_dir(&self) -> PathBuf {
        self.root.join("site")
    }
    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }
    pub fn day_dir(&self, date: &str) -> PathBuf {
        self.days_dir().join(date)
    }
    pub fn snapshot_dir(&self, date: &str, id: &str) -> PathBuf {
        self.day_dir(date).join(id)
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for d in [self.inbox_dir(), self.days_dir(), self.site_dir()] {
            fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
        }
        Ok(())
    }

    pub fn load_config(&self) -> Config {
        match fs::read_to_string(self.config_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save_config(&self, cfg: &Config) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        write_json(&self.config_path(), cfg)
    }

    /// All snapshots, sorted chronologically.
    pub fn list_refs(&self) -> Vec<SnapRef> {
        let mut out = Vec::new();
        let days = match fs::read_dir(self.days_dir()) {
            Ok(d) => d,
            Err(_) => return out,
        };
        for day in days.flatten() {
            let date = day.file_name().to_string_lossy().to_string();
            if !crate::namefmt::is_valid_date_str(&date) || !day.path().is_dir() {
                continue;
            }
            if let Ok(snaps) = fs::read_dir(day.path()) {
                for s in snaps.flatten() {
                    let id = s.file_name().to_string_lossy().to_string();
                    if crate::namefmt::is_valid_time_str(&id) && s.path().is_dir() {
                        out.push(SnapRef {
                            date: date.clone(),
                            id,
                        });
                    }
                }
            }
        }
        out.sort();
        out
    }

    pub fn previous_of(&self, r: &SnapRef) -> Option<SnapRef> {
        self.list_refs().into_iter().filter(|x| x < r).last()
    }

    pub fn next_of(&self, r: &SnapRef) -> Option<SnapRef> {
        self.list_refs().into_iter().find(|x| x > r)
    }

    pub fn meta_path(&self, r: &SnapRef) -> PathBuf {
        self.snapshot_dir(&r.date, &r.id)
            .join("render")
            .join("meta.json")
    }

    pub fn read_meta(&self, r: &SnapRef) -> Option<Meta> {
        let s = fs::read_to_string(self.meta_path(r)).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn diff_path(&self, r: &SnapRef) -> PathBuf {
        self.snapshot_dir(&r.date, &r.id).join("diff.json")
    }

    pub fn read_diff(&self, r: &SnapRef) -> Option<DiffFile> {
        let s = fs::read_to_string(self.diff_path(r)).ok()?;
        serde_json::from_str(&s).ok()
    }

    /// Snapshot with the given zip hash, if one was already imported.
    pub fn find_by_hash(&self, sha256: &str) -> Option<SnapRef> {
        self.list_refs().into_iter().find(|r| {
            self.read_meta(r)
                .map(|m| m.zip_sha256 == sha256)
                .unwrap_or(false)
        })
    }

    pub fn snapshot_infos(&self) -> Vec<SnapshotInfo> {
        self.list_refs()
            .into_iter()
            .map(|r| {
                let dir = self.snapshot_dir(&r.date, &r.id);
                let meta = self.read_meta(&r);
                let diff = self.read_diff(&r);
                let changed = diff
                    .map(|d| {
                        d.dimensions
                            .values()
                            .map(|x| x.added + x.modified + x.removed)
                            .sum::<usize>()
                    })
                    .unwrap_or(0);
                let zip_name = fs::read_dir(dir.join("zip"))
                    .ok()
                    .and_then(|mut it| it.next())
                    .and_then(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string());
                SnapshotInfo {
                    date: r.date.clone(),
                    time: r.id.clone(),
                    local_datetime: meta
                        .as_ref()
                        .map(|m| m.local_datetime.clone())
                        .unwrap_or_default(),
                    utc_offset: meta
                        .as_ref()
                        .map(|m| m.utc_offset.clone())
                        .unwrap_or_default(),
                    dimensions: meta
                        .as_ref()
                        .map(|m| m.dimensions.keys().cloned().collect())
                        .unwrap_or_default(),
                    has_world: dir.join("world").is_dir(),
                    zip_name,
                    tile_count: meta
                        .as_ref()
                        .map(|m| m.dimensions.values().map(|d| d.tile_count).sum::<usize>())
                        .unwrap_or(0),
                    changed_tiles: changed,
                    complete: meta
                        .as_ref()
                        .map(|m| !m.dimensions.is_empty())
                        .unwrap_or(false),
                }
            })
            .collect()
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value)?;
    fs::write(&tmp, data).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let s = fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}
