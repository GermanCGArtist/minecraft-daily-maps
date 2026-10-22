//! The import / re-render / delete pipelines.
//!
//! Steps reported to the UI (in order): `hash`, `extract`,
//! `render:<dimension>` (one per selected dimension), `diff`, `html`.
//! Each step is reported as `start`, then `done` / `skip` / `error`.

use crate::diffcalc::{diff_tiles, Change};
use crate::ingest::{self, WorldLayout};
use crate::model::*;
use crate::render;
use crate::site;
use crate::store::{read_json, write_json, SnapRef, Store};
use crate::tilepath::TileKey;
use crate::timeutil;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// `report(step, state, message)`
pub type Report<'a> = &'a dyn Fn(&str, &str, &str);

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub zip_path: PathBuf,
    pub date: String, // YYYY-MM-DD
    pub time: String, // HHMM
    pub dimensions: Vec<Dimension>,
    /// Move the zip into the snapshot (used for files from `inbox/`) instead
    /// of copying it.
    pub move_zip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    Imported(SnapRef),
    Duplicate(SnapRef),
}

pub fn plan_steps(dims: &[Dimension], with_extract: bool) -> Vec<String> {
    let mut v = Vec::new();
    if with_extract {
        v.push("hash".to_string());
    }
    v.push("extract".to_string());
    for d in dims {
        v.push(format!("render:{}", d.key()));
    }
    v.push("diff".to_string());
    v.push("html".to_string());
    v
}

type RegionManifests = BTreeMap<String, BTreeMap<String, String>>;
type TileHashes = BTreeMap<String, BTreeMap<String, String>>;

fn tile_key_string(k: &TileKey) -> String {
    format!("{}/{}/{}", k.0, k.1, k.2)
}

fn parse_tile_key_string(s: &str) -> Option<TileKey> {
    let mut it = s.split('/');
    let z = it.next()?.parse().ok()?;
    let x = it.next()?.parse().ok()?;
    let y = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((z, x, y))
}

fn to_hash_map(m: &BTreeMap<String, String>) -> HashMap<TileKey, String> {
    m.iter()
        .filter_map(|(k, v)| parse_tile_key_string(k).map(|k| (k, v.clone())))
        .collect()
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(p) = to.parent() {
        fs::create_dir_all(p)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)
        .with_context(|| format!("copying {} -> {}", from.display(), to.display()))?;
    fs::remove_file(from)?;
    Ok(())
}

fn first_zip_in(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case("zip"))
                .unwrap_or(false)
        })
}

/// Render the given dimensions into `<snapshot>/render/tiles/<dim>` and update
/// `meta`, `regions.json` and `tilehashes.json`. When `force` is false, a
/// dimension whose region files are identical to the previous snapshot's
/// reuses that snapshot's tiles instead of calling the renderer.
fn render_dims(
    cfg: &Config,
    store: &Store,
    r: &SnapRef,
    layout: &WorldLayout,
    dims: &[Dimension],
    force: bool,
    meta: &mut Meta,
    report: Report,
) -> Result<()> {
    let snap_dir = store.snapshot_dir(&r.date, &r.id);
    let render_dir = snap_dir.join("render");
    fs::create_dir_all(&render_dir)?;
    let prev = store.previous_of(r);
    let prev_dir = prev.as_ref().map(|p| store.snapshot_dir(&p.date, &p.id));
    let prev_meta = prev.as_ref().and_then(|p| store.read_meta(p));

    let mut regions: RegionManifests =
        read_json(&render_dir.join("regions.json")).unwrap_or_default();
    let mut hashes: TileHashes = read_json(&render_dir.join("tilehashes.json")).unwrap_or_default();

    for dim in dims {
        let step = format!("render:{}", dim.key());
        let world = match layout.world_for(*dim) {
            Some(w) => w.to_path_buf(),
            None => {
                report(&step, "skip", "dimension not present in this world");
                continue;
            }
        };
        report(&step, "start", "");
        let manifest = ingest::region_manifest(&world.join(dim.region_subdir()))?;
        let tiles_dir = render_dir.join("tiles").join(dim.key());

        let mut reused = false;
        if !force {
            if let (Some(pd), Some(pm)) = (prev_dir.as_ref(), prev_meta.as_ref()) {
                let prev_regions: Option<RegionManifests> =
                    read_json(&pd.join("render").join("regions.json"));
                let prev_tiles = pd.join("render").join("tiles").join(dim.key());
                let same_zoom = pm
                    .dimensions
                    .get(dim.key())
                    .map(|d| d.zoom_in == cfg.zoom_in)
                    .unwrap_or(false);
                let same_regions = prev_regions
                    .as_ref()
                    .and_then(|m| m.get(dim.key()))
                    .map(|m| *m == manifest)
                    .unwrap_or(false);
                if same_zoom && same_regions && prev_tiles.is_dir() {
                    report(&step, "log", "no region changed, reusing previous tiles");
                    render::copy_tiles(&prev_tiles, &tiles_dir, cfg.link_unchanged_tiles)?;
                    reused = true;
                }
            }
        }

        if !reused {
            let raw = render_dir.join("_raw").join(dim.key());
            if raw.exists() {
                fs::remove_dir_all(&raw)?;
            }
            render::run_unmined(cfg, &world, &raw, *dim)?;
            render::normalise_tiles(&raw, &tiles_dir)?;
            let _ = fs::remove_dir_all(render_dir.join("_raw"));
        }

        let hmap = render::hash_tiles(&tiles_dir)?;
        let keys: Vec<TileKey> = hmap.keys().copied().collect();
        meta.dimensions.insert(
            dim.key().to_string(),
            render::dimension_meta(&keys, manifest.len(), reused, cfg.zoom_in),
        );
        regions.insert(dim.key().to_string(), manifest);
        hashes.insert(
            dim.key().to_string(),
            hmap.iter()
                .map(|(k, v)| (tile_key_string(k), v.clone()))
                .collect(),
        );
        report(&step, "done", &format!("{} tiles", keys.len()));
    }

    write_json(&render_dir.join("regions.json"), &regions)?;
    write_json(&render_dir.join("tilehashes.json"), &hashes)?;
    meta.rendered_at = timeutil::now_rfc3339();
    write_json(&render_dir.join("meta.json"), &*meta)?;
    Ok(())
}

/// Compare this snapshot's tiles with the previous snapshot in time and
/// write `diff.json`.
pub fn compute_diff(store: &Store, r: &SnapRef) -> Result<()> {
    let meta = match store.read_meta(r) {
        Some(m) => m,
        None => return Ok(()),
    };
    let dir = store.snapshot_dir(&r.date, &r.id);
    let new_hashes: TileHashes =
        read_json(&dir.join("render").join("tilehashes.json")).unwrap_or_default();
    let prev = store.previous_of(r);
    let prev_hashes: Option<TileHashes> = prev.as_ref().and_then(|p| {
        read_json(
            &store
                .snapshot_dir(&p.date, &p.id)
                .join("render")
                .join("tilehashes.json"),
        )
    });

    let mut dims_out: BTreeMap<String, DimensionDiff> = BTreeMap::new();
    for dim in meta.dimensions.keys() {
        let mut dd = DimensionDiff::default();
        let new = new_hashes.get(dim).map(to_hash_map).unwrap_or_default();
        let old = prev_hashes
            .as_ref()
            .and_then(|p| p.get(dim))
            .map(to_hash_map);
        match old {
            None => dd.baseline = true,
            Some(old) => {
                for ((z, x, y), c) in diff_tiles(&old, &new) {
                    match c {
                        Change::Added => dd.added += 1,
                        Change::Modified => dd.modified += 1,
                        Change::Removed => dd.removed += 1,
                    }
                    dd.tiles
                        .entry(z.to_string())
                        .or_default()
                        .push((x, y, c.code().to_string()));
                }
            }
        }
        dims_out.insert(dim.clone(), dd);
    }
    let diff = DiffFile {
        schema: SCHEMA_VERSION,
        against: prev.map(|p| SnapshotId {
            date: p.date,
            time: p.id,
        }),
        dimensions: dims_out,
    };
    write_json(&store.diff_path(r), &diff)
}

/// Import a world zip as a new snapshot.
pub fn import(
    cfg: &Config,
    store: &Store,
    req: &ImportRequest,
    report: Report,
) -> Result<ImportOutcome> {
    store.ensure_layout()?;
    let (local_datetime, utc_offset) = timeutil::local_datetime_info(&req.date, &req.time)?;
    if req.dimensions.is_empty() {
        bail!("Select at least one dimension to render.");
    }

    report("hash", "start", "");
    let sha = ingest::sha256_file(&req.zip_path)?;
    if let Some(existing) = store.find_by_hash(&sha) {
        report(
            "hash",
            "skip",
            &format!("already imported as {} {}", existing.date, existing.id),
        );
        return Ok(ImportOutcome::Duplicate(existing));
    }
    report("hash", "done", "");

    let r = SnapRef {
        date: req.date.clone(),
        id: req.time.clone(),
    };
    let dir = store.snapshot_dir(&r.date, &r.id);
    if dir.exists() {
        bail!(
            "A snapshot for {} {} already exists. Pick another time or delete the existing snapshot first.",
            r.date,
            r.id
        );
    }
    let zip_dir = dir.join("zip");
    fs::create_dir_all(&zip_dir)?;
    let zip_name = req
        .zip_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "world.zip".to_string());
    let zip_dest = zip_dir.join(&zip_name);
    if req.move_zip {
        move_file(&req.zip_path, &zip_dest)?;
    } else {
        fs::copy(&req.zip_path, &zip_dest)
            .with_context(|| format!("copying {}", req.zip_path.display()))?;
    }

    // Preliminary meta: makes the hash visible for duplicate detection even if
    // a later step fails and the snapshot has to be re-rendered.
    let mut meta = Meta {
        schema: SCHEMA_VERSION,
        date: r.date.clone(),
        time: r.id.clone(),
        local_datetime,
        utc_offset,
        source_zip: zip_name,
        zip_sha256: sha,
        rendered_at: timeutil::now_rfc3339(),
        dimensions: BTreeMap::new(),
    };
    write_json(&dir.join("render").join("meta.json"), &meta)?;

    report("extract", "start", "");
    let world_dir = dir.join("world");
    ingest::extract_zip(&zip_dest, &world_dir)?;
    let layout = ingest::locate_worlds(&world_dir);
    if layout.available().is_empty() {
        bail!("No Minecraft region files (.mca) were found in this zip.");
    }
    report("extract", "done", "");

    let selected: Vec<Dimension> = req
        .dimensions
        .iter()
        .copied()
        .filter(|d| layout.world_for(*d).is_some())
        .collect();
    for d in &req.dimensions {
        if !selected.contains(d) {
            report(
                &format!("render:{}", d.key()),
                "skip",
                "dimension not present in this world",
            );
        }
    }
    if selected.is_empty() {
        bail!("None of the selected dimensions exist in this world.");
    }
    render_dims(cfg, store, &r, &layout, &selected, false, &mut meta, report)?;

    report("diff", "start", "");
    compute_diff(store, &r)?;
    if let Some(next) = store.next_of(&r) {
        // Importing a snapshot out of order changes what its successor is compared with.
        compute_diff(store, &next)?;
    }
    report("diff", "done", "");

    report("html", "start", "");
    site::rebuild_day(store, &r.date)?;
    if let Some(next) = store.next_of(&r) {
        if next.date != r.date {
            site::rebuild_day(store, &next.date)?;
        }
    }
    site::rebuild_index(store)?;
    report("html", "done", "");

    if !cfg.keep_world {
        let _ = fs::remove_dir_all(&world_dir);
    }
    Ok(ImportOutcome::Imported(r))
}

/// Re-render selected dimensions of an existing snapshot.
pub fn rerender(
    cfg: &Config,
    store: &Store,
    r: &SnapRef,
    dims: &[Dimension],
    report: Report,
) -> Result<()> {
    let mut meta = store
        .read_meta(r)
        .ok_or_else(|| anyhow!("Snapshot {} {} has no metadata.", r.date, r.id))?;
    if dims.is_empty() {
        bail!("Select at least one dimension to render.");
    }
    let dir = store.snapshot_dir(&r.date, &r.id);
    let world_dir = dir.join("world");
    let had_world = world_dir.is_dir();

    report("extract", "start", "");
    if !had_world {
        let zip = first_zip_in(&dir.join("zip")).ok_or_else(|| {
            anyhow!(
                "The original zip is gone and the extracted world was deleted; cannot re-render."
            )
        })?;
        ingest::extract_zip(&zip, &world_dir)?;
    }
    let layout = ingest::locate_worlds(&world_dir);
    report(
        "extract",
        "done",
        if had_world {
            "using existing world folder"
        } else {
            ""
        },
    );

    let selected: Vec<Dimension> = dims
        .iter()
        .copied()
        .filter(|d| layout.world_for(*d).is_some())
        .collect();
    for d in dims {
        if !selected.contains(d) {
            report(
                &format!("render:{}", d.key()),
                "skip",
                "dimension not present in this world",
            );
        }
    }
    if selected.is_empty() {
        bail!("None of the selected dimensions exist in this world.");
    }
    render_dims(cfg, store, r, &layout, &selected, true, &mut meta, report)?;

    report("diff", "start", "");
    compute_diff(store, r)?;
    let next = store.next_of(r);
    if let Some(n) = &next {
        compute_diff(store, n)?;
    }
    report("diff", "done", "");

    report("html", "start", "");
    site::rebuild_day(store, &r.date)?;
    if let Some(n) = &next {
        if n.date != r.date {
            site::rebuild_day(store, &n.date)?;
        }
    }
    site::rebuild_index(store)?;
    report("html", "done", "");

    if !cfg.keep_world {
        let _ = fs::remove_dir_all(&world_dir);
    }
    Ok(())
}

/// Delete a snapshot folder and repair what depended on it.
pub fn delete_snapshot(store: &Store, r: &SnapRef) -> Result<()> {
    let next = store.next_of(r);
    let dir = store.snapshot_dir(&r.date, &r.id);
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("deleting {}", dir.display()))?;
    }
    // Drop the day folder when it has no snapshots left.
    let day = store.day_dir(&r.date);
    let day_has_snapshots = fs::read_dir(&day)
        .map(|it| it.flatten().any(|e| e.path().is_dir()))
        .unwrap_or(false);
    if !day_has_snapshots && day.exists() {
        fs::remove_dir_all(&day)?;
    }
    if let Some(n) = &next {
        compute_diff(store, n)?;
        site::rebuild_day(store, &n.date)?;
    }
    if day.exists() {
        site::rebuild_day(store, &r.date)?;
    }
    site::rebuild_index(store)
}
