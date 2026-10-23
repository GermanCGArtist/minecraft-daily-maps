//! The import pipeline: hash, copy, extract, render per dimension, index tiles, diff, build HTML.

use crate::{
    error::{Error, Result},
    extract, naming, native, site,
    model::{Config, Dimension, Incomplete, SnapshotMeta, TilesManifest},
    store::{read_json, write_json, Store},
    tiles, hashing,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Tile edge in pixels (one level-0 tile is one 512 x 512 block region).
pub const TILE_SIZE: u32 = native::TILE_SIZE;

/// `(step, detail, overall fraction 0..1)`
pub type Progress<'a> = &'a dyn Fn(&str, &str, f32);

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub zip_path: PathBuf,
    /// `YYYY-MM-DD`, local
    pub date: String,
    /// `HHMM`, local
    pub time: String,
    pub dimensions: Vec<Dimension>,
    /// Delete the source zip after a successful import (used for the inbox).
    pub move_source: bool,
}

pub fn import(store: &Store, req: &ImportRequest, progress: Progress) -> Result<SnapshotMeta> {
    let (date, time) = naming::validate_stamp(&req.date, &req.time)
        .ok_or_else(|| Error::Invalid("Date must be YYYY-MM-DD and time HHMM (e.g. 0005).".into()))?;
    if req.dimensions.is_empty() {
        return Err(Error::Invalid("Select at least one dimension to render.".into()));
    }
    if !req.zip_path.is_file() {
        return Err(Error::Invalid(format!("{} is not a file.", req.zip_path.display())));
    }
    let cfg = store.load_config()?;

    progress("hash", "Hashing zip", 0.01);
    let zip_hash = hashing::hash_file_parallel(&req.zip_path)?;
    if let Some(existing) = store.find_by_zip_hash(&zip_hash)? {
        return Err(Error::Duplicate(existing.id()));
    }

    if let Some(inc) = store.find_incomplete_by_hash(&zip_hash)? {
        return Err(Error::Invalid(format!(
            "This zip has an unfinished import ({} {}). Resume or discard it from 'Unfinished imports'.",
            inc.date, inc.time
        )));
    }

    let dir = store.snapshot_dir(&date, &time);
    if dir.exists() {
        return Err(Error::Invalid(format!(
            "A snapshot already exists for {date} {time}. Change the time and try again."
        )));
    }

    match import_inner(store, &cfg, req, &date, &time, &zip_hash, &dir, progress) {
        Ok(meta) => {
            if req.move_source {
                let _ = fs::remove_file(&req.zip_path);
            }
            Ok(meta)
        }
        Err(e) => {
            // Once the world is extracted, a render or disk failure keeps the tiles drawn so far
            // (a resumed render only draws the regions that are still missing).
            if dir.join("incomplete.json").is_file() && is_resumable(&e) {
                mark_failed(&dir, &e);
            } else {
                let _ = fs::remove_dir_all(&dir);
            }
            Err(e)
        }
    }
}

fn is_resumable(e: &Error) -> bool {
    matches!(e, Error::Renderer(_) | Error::Io(_) | Error::Json(_))
}

fn mark_failed(dir: &Path, e: &Error) {
    if let Ok(Some(mut inc)) = read_json::<Incomplete>(&dir.join("incomplete.json")) {
        inc.last_error = e.to_string();
        let _ = write_json(&dir.join("incomplete.json"), &inc);
    }
}

/// Make sure the world is extracted in `dir/world` (re-extracting from the kept zip if needed).
fn ensure_world(dir: &Path, progress: Progress) -> Result<PathBuf> {
    let world_dir = dir.join("world");
    if !world_dir.is_dir() {
        let zip = fs::read_dir(dir.join("zip"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().map_or(false, |x| x.eq_ignore_ascii_case("zip")))
            .ok_or_else(|| Error::Invalid("The snapshot's zip is missing, cannot re-extract the world.".into()))?;
        extract::extract_zip(&zip, &world_dir, &|f| progress("extract", "Extracting world", 0.02 + 0.28 * f))?;
    }
    Ok(world_dir)
}

#[allow(clippy::too_many_arguments)]
fn import_inner(
    store: &Store,
    cfg: &Config,
    req: &ImportRequest,
    date: &str,
    time: &str,
    zip_hash: &str,
    dir: &Path,
    progress: Progress,
) -> Result<SnapshotMeta> {
    let zip_dir = dir.join("zip");
    fs::create_dir_all(&zip_dir)?;
    let file_name = req.zip_path.file_name().map(|n| n.to_owned()).unwrap_or_else(|| "world.zip".into());
    progress("copy", "Copying zip into the snapshot", 0.03);
    fs::copy(&req.zip_path, zip_dir.join(&file_name))?;

    let world_dir = dir.join("world");
    extract::extract_zip(&req.zip_path, &world_dir, &|f| {
        progress("extract", "Extracting world", 0.05 + 0.25 * f)
    })?;
    let world = extract::find_world_root(&world_dir)?;

    // From here on a failure keeps the folder so the import can be resumed.
    let inc = Incomplete {
        date: date.to_string(),
        time: time.to_string(),
        source_name: file_name.to_string_lossy().to_string(),
        zip_hash: zip_hash.to_string(),
        dimensions: req.dimensions.clone(),
        created_at: chrono::Local::now().to_rfc3339(),
        last_error: String::new(),
    };
    write_json(&dir.join("incomplete.json"), &inc)?;
    finish_snapshot(store, cfg, dir, &world_dir, &world, &inc, progress)
}

/// Render the requested dimensions, index tiles, write `meta.json`, build diff and pages.
/// Shared by a normal import and by resuming an unfinished one.
fn finish_snapshot(
    store: &Store,
    cfg: &Config,
    dir: &Path,
    world_dir: &Path,
    world: &Path,
    inc: &Incomplete,
    progress: Progress,
) -> Result<SnapshotMeta> {
    let prev = store.previous_snapshot(&inc.date, &inc.time)?;
    let done = render_dimensions(store, cfg, dir, world, &inc.dimensions, prev.as_ref(), progress, (0.30, 0.85))?;
    let present = write_tiles_manifest(&dir.join("render"), &done)?;

    let meta = SnapshotMeta {
        date: inc.date.clone(),
        time: inc.time.clone(),
        utc_offset: naming::local_utc_offset(&inc.date, &inc.time),
        source_name: inc.source_name.clone(),
        zip_hash: inc.zip_hash.clone(),
        dimensions: present,
        zoom_in: 0,
        tile_size: TILE_SIZE,
        renderer: native::RENDERER_ID.to_string(),
        world_kept: cfg.keep_world,
        created_at: chrono::Local::now().to_rfc3339(),
    };

    if !cfg.keep_world {
        let _ = fs::remove_dir_all(world_dir);
    }
    write_json(&dir.join("render").join("meta.json"), &meta)?;
    let _ = fs::remove_file(dir.join("incomplete.json"));

    progress("html", "Building diff and pages", 0.90);
    site::rebuild(store, &[meta.id()])?;
    progress("done", "Done", 1.0);
    Ok(meta)
}

/// Continue an unfinished import: regions that already have a tile are not drawn again.
pub fn resume(store: &Store, date: &str, time: &str, progress: Progress) -> Result<SnapshotMeta> {
    let cfg = store.load_config()?;
    let dir = store.snapshot_dir(date, time);
    let inc: Incomplete = read_json(&dir.join("incomplete.json"))?
        .ok_or_else(|| Error::Invalid(format!("No unfinished import found at {date} {time}.")))?;

    let result = (|| -> Result<SnapshotMeta> {
        let world_dir = ensure_world(&dir, progress)?;
        let world = extract::find_world_root(&world_dir)?;
        finish_snapshot(store, &cfg, &dir, &world_dir, &world, &inc, progress)
    })();
    if let Err(e) = &result {
        if is_resumable(e) {
            mark_failed(&dir, e);
        }
    }
    result
}

/// Render the chosen dimensions of an existing snapshot again (e.g. a dimension that was
/// skipped before). Regions whose tile is already up to date are not drawn again.
pub fn rerender(
    store: &Store,
    date: &str,
    time: &str,
    dimensions: &[Dimension],
    progress: Progress,
) -> Result<SnapshotMeta> {
    if dimensions.is_empty() {
        return Err(Error::Invalid("Select at least one dimension to render.".into()));
    }
    let cfg = store.load_config()?;
    let dir = store.snapshot_dir(date, time);
    let mut meta: SnapshotMeta = read_json(&dir.join("render").join("meta.json"))?
        .ok_or_else(|| Error::Invalid(format!("Snapshot {date} {time} not found.")))?;

    // Tiles from another renderer (uNmINeD, v0.1 - v0.3) cannot be mixed with new ones in one
    // snapshot, so in that case every dimension of the snapshot is drawn again.
    let mut dims = dimensions.to_vec();
    if meta.renderer != native::RENDERER_ID {
        for d in &meta.dimensions {
            if !dims.contains(d) {
                dims.push(*d);
            }
        }
    }

    let world_dir = ensure_world(&dir, progress)?;
    let world = extract::find_world_root(&world_dir)?;
    let prev = store.previous_snapshot(date, time)?;

    let result = render_dimensions(store, &cfg, &dir, &world, &dims, prev.as_ref(), progress, (0.30, 0.85))
        .and_then(|done| write_tiles_manifest(&dir.join("render"), &done));

    if !cfg.keep_world {
        let _ = fs::remove_dir_all(&world_dir);
    }
    let present = result?;

    meta.dimensions = present;
    meta.zoom_in = 0;
    meta.tile_size = TILE_SIZE;
    meta.renderer = native::RENDERER_ID.to_string();
    meta.world_kept = cfg.keep_world;
    write_json(&dir.join("render").join("meta.json"), &meta)?;

    progress("html", "Building diff and pages", 0.90);
    site::rebuild(store, &[meta.id()])?;
    progress("done", "Done", 1.0);
    Ok(meta)
}

pub fn delete_snapshot(store: &Store, date: &str, time: &str) -> Result<()> {
    if naming::validate_stamp(date, time).is_none() {
        return Err(Error::Invalid("Invalid snapshot id.".into()));
    }
    let dir = store.snapshot_dir(date, time);
    if dir.is_dir() {
        fs::remove_dir_all(&dir)?;
    }
    let day = store.days_dir().join(date);
    let has_snapshots = fs::read_dir(&day)
        .map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.path().is_dir()))
        .unwrap_or(false);
    if !has_snapshots && day.is_dir() {
        fs::remove_dir_all(&day)?;
    }
    site::rebuild(store, &[])
}

#[allow(clippy::too_many_arguments)]
fn render_dimensions(
    store: &Store,
    cfg: &Config,
    dir: &Path,
    world: &Path,
    dims: &[Dimension],
    prev: Option<&SnapshotMeta>,
    progress: Progress,
    range: (f32, f32),
) -> Result<Vec<Dimension>> {
    let render_dir = dir.join("render");
    fs::create_dir_all(&render_dir)?;
    // Tiles of the previous snapshot are only reused when the same renderer drew them.
    let prev_dir = prev
        .filter(|p| p.renderer == native::RENDERER_ID && p.tile_size == TILE_SIZE)
        .map(|p| store.snapshot_dir(&p.date, &p.time));

    let n = dims.len().max(1) as f32;
    let mut done = Vec::new();
    for (i, &dim) in dims.iter().enumerate() {
        let lo = range.0 + (range.1 - range.0) * i as f32 / n;
        let hi = range.0 + (range.1 - range.0) * (i + 1) as f32 / n;
        let name = dim.as_str();

        let Some(rdir) = extract::region_dir(world, dim) else {
            progress("render", &format!("{name}: not present in this world, skipped"), lo);
            continue;
        };

        progress("hash", &format!("{name}: hashing region files"), lo);
        let hashes = extract::hash_regions(&rdir)?;

        let out = render_dir.join("tiles").join(name);
        if !out.is_dir() {
            // Start from the previous snapshot's tiles (hard links): only changed regions are redrawn.
            if let Some(po) = prev_dir.as_ref().map(|d| d.join("render").join("tiles").join(name)).filter(|p| p.is_dir()) {
                progress("seed", &format!("{name}: reusing previous tiles"), lo);
                extract::link_or_copy_dir(&po, &out)?;
            }
        }

        let span = (hi - lo) * 0.9;
        native::render_dimension(cfg, dim, &rdir, &hashes, &out, &|line, f| {
            progress("render", &format!("{name}: {line}"), lo + span * f)
        })?;

        if cfg.stitched_image {
            let png = render_dir.join("stitched").join(format!("{name}.png"));
            native::stitch(&out, &png, cfg.stitched_max_px, &|line, _| {
                progress("stitch", &format!("{name}: {line}"), lo + span)
            })?;
        }
        progress("render", &format!("{name}: done"), hi);

        done.push(dim);
    }
    Ok(done)
}

/// Index every dimension folder under `render/tiles/` into `render/tiles.json`.
/// Fails loudly if a dimension that was just rendered yields no recognisable tiles.
fn write_tiles_manifest(render_dir: &Path, just_rendered: &[Dimension]) -> Result<Vec<Dimension>> {
    let mut manifest =
        TilesManifest { tile_size: TILE_SIZE, renderer: native::RENDERER_ID.to_string(), dimensions: BTreeMap::new() };
    for dim in Dimension::ALL {
        let d = render_dir.join("tiles").join(dim.as_str());
        if d.is_dir() {
            let t = tiles::index_tiles(&d)?;
            if !t.tiles.is_empty() {
                manifest.dimensions.insert(dim, t);
            }
        }
    }
    for dim in just_rendered {
        if !manifest.dimensions.contains_key(dim) {
            return Err(Error::Renderer(format!(
                "no tiles were produced for {}: none of its region files contain generated chunks. Look in {}.",
                dim.as_str(),
                render_dir.join("tiles").join(dim.as_str()).display()
            )));
        }
    }
    if manifest.dimensions.is_empty() {
        return Err(Error::Invalid(
            "None of the selected dimensions exist in this world (no region folders found).".into(),
        ));
    }
    write_json(&render_dir.join("tiles.json"), &manifest)?;
    Ok(manifest.dimensions.keys().copied().collect())
}
