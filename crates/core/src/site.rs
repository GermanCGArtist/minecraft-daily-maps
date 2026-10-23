//! Builds everything the browser sees: diffs, per-snapshot pages, the day-slider site.
//!
//! All data the pages need is written as `.js` files that assign to `window.*`, because
//! `fetch()` of local JSON does not work from `file://` pages. The viewer therefore opens
//! with a double click, and a snapshot folder can be zipped and shared on its own.

use crate::{
    diff,
    error::Result,
    model::{DiffFile, Dimension, SnapshotMeta, TilesManifest},
    store::{read_json, write_json, Store},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
};

const VIEWER_JS: &str = include_str!("../assets/viewer.js");
const VIEWER_CSS: &str = include_str!("../assets/viewer.css");
const INDEX_HTML: &str = include_str!("../assets/index.html");

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

fn page(title: &str, data_script: &str) -> String {
    INDEX_HTML.replace("{{TITLE}}", title).replace("{{DATA}}", data_script)
}

/// Tile manifest in the compact form the viewer wants: per dimension, per zoom level,
/// `[x, y, path]` rows (hashes are dropped).
fn viewer_tiles(m: &TilesManifest) -> Value {
    let mut dims = serde_json::Map::new();
    for (dim, dt) in &m.dimensions {
        let mut levels: BTreeMap<i32, Vec<Value>> = BTreeMap::new();
        for (z, x, y, path, _) in &dt.tiles {
            levels.entry(*z).or_default().push(json!([x, y, path]));
        }
        let levels: Vec<Value> = levels.into_iter().map(|(z, t)| json!({ "z": z, "tiles": t })).collect();
        dims.insert(dim.as_str().to_string(), json!({ "levels": levels }));
    }
    json!({ "tile_size": m.tile_size, "dimensions": dims })
}

fn diff_value(dims: &BTreeMap<Dimension, crate::model::DimDiff>) -> Value {
    let mut map = serde_json::Map::new();
    for (dim, d) in dims {
        map.insert(dim.as_str().to_string(), serde_json::to_value(d).unwrap());
    }
    Value::Object(map)
}

struct Entry {
    meta: SnapshotMeta,
    manifest: Option<TilesManifest>,
    diff: DiffFile,
}

/// Recompute whatever is stale and rewrite all pages.
/// `touched` lists snapshot ids (`2026-10-22/0005`) that were just created or re-rendered.
pub fn rebuild(store: &Store, touched: &[String]) -> Result<()> {
    let snaps = store.list_snapshots()?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut prev: Option<(String, TilesManifest)> = None;

    for meta in snaps {
        let dir = store.snapshot_dir(&meta.date, &meta.time);
        let id = meta.id();
        let manifest: Option<TilesManifest> = read_json(&dir.join("render").join("tiles.json"))?;
        let prev_id = prev.as_ref().map(|(i, _)| i.clone());

        let existing: Option<DiffFile> = read_json(&dir.join("diff.json"))?;
        let stale = existing.is_none()
            || touched.contains(&id)
            || prev_id.as_ref().map_or(false, |p| touched.contains(p))
            || existing.as_ref().map(|d| &d.previous) != Some(&prev_id);

        let diff = match (stale, existing) {
            (false, Some(d)) => d,
            _ => {
                let d = match (&prev, &manifest) {
                    (Some((pid, pm)), Some(cm)) => diff::compute(pm, cm, Some(pid.clone())),
                    _ => DiffFile { previous: prev_id.clone(), dimensions: BTreeMap::new() },
                };
                write_json(&dir.join("diff.json"), &d)?;
                d
            }
        };

        if let Some(m) = &manifest {
            prev = Some((id, m.clone()));
        }
        entries.push(Entry { meta, manifest, diff });
    }

    // Group by day for siblings and the site.
    let mut by_day: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        by_day.entry(e.meta.date.clone()).or_default().push(i);
    }

    // Per-snapshot pages.
    for e in &entries {
        let dir = store.snapshot_dir(&e.meta.date, &e.meta.time);
        let id = e.meta.id();
        let siblings: Vec<Value> = by_day[&e.meta.date]
            .iter()
            .map(|&i| {
                let s = &entries[i].meta;
                json!({ "time": s.time, "href": format!("../{}/index.html", s.time) })
            })
            .collect();

        let dims: Vec<&str> = e.meta.dimensions.iter().map(|d| d.as_str()).collect();
        let view = json!({
            "mode": "snapshot",
            "id": id,
            "date": e.meta.date,
            "time": e.meta.time,
            "utc_offset": e.meta.utc_offset,
            "dimensions": dims,
            "tile_size": e.meta.tile_size,
            "siblings": siblings,
        });
        write_text(&dir.join("viewer-data.js"), &format!("window.MC_VIEW = {};\n", view))?;

        if let Some(m) = &e.manifest {
            let tiles_js = dir.join("render").join("tiles-data.js");
            if touched.contains(&id) || !tiles_js.exists() {
                write_text(
                    &tiles_js,
                    &format!(
                        "(window.MC_TILES = window.MC_TILES || {{}})[{}] = {};\n",
                        serde_json::to_string(&id)?,
                        viewer_tiles(m)
                    ),
                )?;
            }
        }
        write_text(
            &dir.join("diff-data.js"),
            &format!(
                "(window.MC_DIFF = window.MC_DIFF || {{}})[{}] = {};\n",
                serde_json::to_string(&id)?,
                diff_value(&e.diff.dimensions)
            ),
        )?;

        write_text(&dir.join("viewer.js"), VIEWER_JS)?;
        write_text(&dir.join("viewer.css"), VIEWER_CSS)?;
        write_text(
            &dir.join("index.html"),
            &page(&format!("Map {} {}", e.meta.date, e.meta.time), "viewer-data.js"),
        )?;
    }

    // latest.json per day + the site.
    let mut days_json: Vec<Value> = Vec::new();
    for (date, idxs) in &by_day {
        let times: Vec<&str> = idxs.iter().map(|&i| entries[i].meta.time.as_str()).collect();
        let latest = *times.last().unwrap();
        let day_dir = store.days_dir().join(date);
        write_json(&day_dir.join("latest.json"), &json!({ "date": date, "latest": latest, "snapshots": times }))?;

        let latest_entry = &entries[*idxs.last().unwrap()];
        let changes = diff::union(idxs.iter().map(|&i| &entries[i].diff));
        days_json.push(json!({
            "date": date,
            "latest": latest,
            "snapshots": idxs.iter().map(|&i| {
                let m = &entries[i].meta;
                json!({ "id": m.id(), "time": m.time, "dimensions": m.dimensions.iter().map(|d| d.as_str()).collect::<Vec<_>>() })
            }).collect::<Vec<_>>(),
            "dimensions": latest_entry.meta.dimensions.iter().map(|d| d.as_str()).collect::<Vec<_>>(),
            "changes": diff_value(&changes),
        }));
    }
    let tile_size = entries.last().map(|e| e.meta.tile_size).unwrap_or(256);
    let site = store.site_dir();
    let view = json!({ "mode": "site", "tile_size": tile_size, "days": days_json });
    write_json(&site.join("days.json"), &view)?;
    write_text(&site.join("days-data.js"), &format!("window.MC_VIEW = {};\n", view))?;
    write_text(&site.join("viewer.js"), VIEWER_JS)?;
    write_text(&site.join("viewer.css"), VIEWER_CSS)?;
    write_text(&site.join("index.html"), &page("Minecraft Daily Maps", "days-data.js"))?;
    Ok(())
}
