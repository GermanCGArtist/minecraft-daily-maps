//! The data folder: `inbox/`, `days/`, `site/`, `config.json`.

use crate::{
    error::Result,
    model::{Config, Incomplete, SnapshotMeta},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    /// Open (and create if needed) a data folder.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let store = Store { root: root.into() };
        fs::create_dir_all(store.inbox_dir())?;
        fs::create_dir_all(store.days_dir())?;
        fs::create_dir_all(store.site_dir())?;
        Ok(store)
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
    pub fn snapshot_dir(&self, date: &str, time: &str) -> PathBuf {
        self.days_dir().join(date).join(time)
    }

    pub fn load_config(&self) -> Result<Config> {
        match fs::read(self.config_path()) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_config(&self, cfg: &Config) -> Result<()> {
        write_json(&self.config_path(), cfg)
    }

    /// All complete snapshots (those with a `render/meta.json`), oldest first.
    pub fn list_snapshots(&self) -> Result<Vec<SnapshotMeta>> {
        let mut out = Vec::new();
        let Ok(days) = fs::read_dir(self.days_dir()) else { return Ok(out) };
        for day in days.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
            for snap in fs::read_dir(day.path())?.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
                let meta_path = snap.path().join("render").join("meta.json");
                if let Ok(bytes) = fs::read(&meta_path) {
                    if let Ok(meta) = serde_json::from_slice::<SnapshotMeta>(&bytes) {
                        out.push(meta);
                    }
                }
            }
        }
        out.sort_by(|a, b| (&a.date, &a.time).cmp(&(&b.date, &b.time)));
        Ok(out)
    }

    /// Snapshot folders whose import failed part-way (no `meta.json`, but an `incomplete.json`).
    pub fn list_incomplete(&self) -> Result<Vec<Incomplete>> {
        let mut out = Vec::new();
        let Ok(days) = fs::read_dir(self.days_dir()) else { return Ok(out) };
        for day in days.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
            let Ok(snaps) = fs::read_dir(day.path()) else { continue };
            for snap in snaps.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
                let p = snap.path();
                if p.join("render").join("meta.json").is_file() {
                    continue;
                }
                if let Ok(bytes) = fs::read(p.join("incomplete.json")) {
                    if let Ok(inc) = serde_json::from_slice::<Incomplete>(&bytes) {
                        out.push(inc);
                    }
                }
            }
        }
        out.sort_by(|a, b| (&a.date, &a.time).cmp(&(&b.date, &b.time)));
        Ok(out)
    }

    pub fn find_incomplete_by_hash(&self, hash: &str) -> Result<Option<Incomplete>> {
        Ok(self.list_incomplete()?.into_iter().find(|i| i.zip_hash == hash))
    }

    pub fn find_by_zip_hash(&self, hash: &str) -> Result<Option<SnapshotMeta>> {
        Ok(self.list_snapshots()?.into_iter().find(|m| m.zip_hash == hash))
    }

    /// The latest snapshot strictly before the given local date and time.
    pub fn previous_snapshot(&self, date: &str, time: &str) -> Result<Option<SnapshotMeta>> {
        Ok(self
            .list_snapshots()?
            .into_iter()
            .filter(|m| (m.date.as_str(), m.time.as_str()) < (date, time))
            .last())
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
