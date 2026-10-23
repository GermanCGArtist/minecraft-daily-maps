//! The map renderer: region files in, tiles out.
//!
//! * Level 0 holds one 512 x 512 PNG per region file (`zoom.0/t.X.Z.png`, one pixel per block).
//! * Every further level (`zoom.-1`, `zoom.-2`, ...) halves the resolution: a tile is made from
//!   the four tiles below it, until the whole map fits into 2 x 2 tiles around the origin.
//! * Each dimension folder also holds `sources.json`: which region file (by hash) each level-0
//!   tile was drawn from. That is what makes rendering incremental and resumable: a region whose
//!   hash did not change keeps its tile, and a crashed run continues where it stopped.
//!
//! Tiles are always written to a temporary file and renamed, never modified in place. Tiles of
//! a new snapshot start out as hard links to the previous snapshot's tiles (see
//! `extract::link_or_copy_dir`), so that is required for them not to change the old snapshot.

use crate::{
    anvil::RegionFile,
    error::{Error, Result},
    model::{Config, Dimension},
    store::read_json,
    topdown,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Instant,
};

/// Bump this whenever the look of the tiles changes (colours, shading, tile size). Tiles of
/// another renderer version are never reused and never compared in the diff.
pub const RENDERER_ID: &str = "native-1";
/// Pixels per tile edge. One level-0 tile is one region: 512 blocks, 512 pixels.
pub const TILE_SIZE: u32 = 512;
const MIN_ZOOM: i32 = -12;
/// Upper limit of the stitched image (RGBA, 4 bytes per pixel).
const MAX_STITCH_PIXELS: u64 = 250_000_000;

/// Receives a line of text and the progress of this dimension (0..1).
pub type Log<'a> = &'a dyn Fn(&str, f32);

#[derive(Debug, Default, Clone)]
pub struct DimStats {
    pub rendered: usize,
    pub empty: usize,
    pub reused: usize,
    pub failed: usize,
}

#[derive(Serialize, Deserialize, Default)]
struct Journal {
    renderer: String,
    /// Region file name -> hash of that file when its tile was drawn.
    /// `"<hash>:empty"` means the region has nothing to draw.
    regions: BTreeMap<String, String>,
}

fn tile_path(dim_dir: &Path, z: i32, x: i32, y: i32) -> PathBuf {
    dim_dir.join(format!("zoom.{z}")).join(format!("t.{x}.{y}.png"))
}

/// `r.-3.5.mca` -> (-3, 5)
fn parse_region_name(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

/// `t.-3.5.png` -> (-3, 5)
fn parse_tile_file(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("t.")?.strip_suffix(".png")?;
    let (x, y) = rest.split_once('.')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

fn list_level(dim_dir: &Path, z: i32) -> Result<BTreeSet<(i32, i32)>> {
    let mut set = BTreeSet::new();
    let rd = match fs::read_dir(dim_dir.join(format!("zoom.{z}"))) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(set),
        Err(e) => return Err(e.into()),
    };
    for e in rd {
        let e = e?;
        if let Some(xy) = e.file_name().to_str().and_then(parse_tile_file) {
            set.insert(xy);
        }
    }
    Ok(set)
}

fn tmp_name(path: &Path) -> PathBuf {
    let mut t = path.as_os_str().to_owned();
    t.push(".tmp");
    PathBuf::from(t)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_name(path);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn write_png(path: &Path, w: u32, h: u32, rgba: Vec<u8>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| Error::Renderer("internal error: image buffer has the wrong size".into()))?;
    let tmp = tmp_name(path);
    img.save_with_format(&tmp, image::ImageFormat::Png)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn read_png(path: &Path) -> Result<Vec<u8>> {
    let img = image::open(path)?.to_rgba8();
    if img.width() != TILE_SIZE || img.height() != TILE_SIZE {
        return Err(Error::Renderer(format!("tile {} has an unexpected size", path.display())));
    }
    Ok(img.into_raw())
}

fn build_pool(cfg: &Config) -> Result<rayon::ThreadPool> {
    let n = if cfg.render_threads > 0 {
        cfg.render_threads as usize
    } else {
        // Leave one core free so the computer stays usable while a map renders.
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).saturating_sub(1).max(1)
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .map_err(|e| Error::Renderer(format!("could not start render threads: {e}")))
}

struct Job {
    name: String,
    hash: String,
    rx: i32,
    rz: i32,
}

enum Kind {
    Tile,
    Empty,
    Failed(String),
}

struct Outcome {
    name: String,
    hash: String,
    kind: Kind,
    unknown: BTreeSet<String>,
    note: Option<String>,
}

fn render_job(job: &Job, region_dir: &Path, out: &Path, nether: bool) -> Outcome {
    let mk = |kind: Kind, unknown: BTreeSet<String>, note: Option<String>| Outcome {
        name: job.name.clone(),
        hash: job.hash.clone(),
        kind,
        unknown,
        note,
    };
    let region = match RegionFile::open(&region_dir.join(&job.name)) {
        Ok(r) => r,
        Err(e) => return mk(Kind::Failed(format!("cannot read file: {e}")), BTreeSet::new(), None),
    };
    let r = topdown::render_region(&region, nether);
    drop(region);

    if r.chunks_ok == 0 && r.chunks_bad > 0 {
        let why = r.first_error.unwrap_or_else(|| "unreadable chunks".into());
        return mk(Kind::Failed(why), r.unknown, None);
    }
    let note = if r.chunks_bad > 0 {
        Some(format!("{} damaged chunk(s) skipped ({})", r.chunks_bad, r.first_error.unwrap_or_default()))
    } else {
        None
    };
    if !r.has_pixels {
        return mk(Kind::Empty, r.unknown, note);
    }
    match write_png(&tile_path(out, 0, job.rx, job.rz), TILE_SIZE, TILE_SIZE, r.rgba) {
        Ok(()) => mk(Kind::Tile, r.unknown, note),
        Err(e) => mk(Kind::Failed(e.to_string()), r.unknown, note),
    }
}

fn clear_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;
    Ok(())
}

fn save_journal(path: &Path, j: &Journal) -> Result<()> {
    write_atomic(path, &serde_json::to_vec(j)?)
}

/// Render every region of one dimension into `out` (tiles, pyramid, journal).
/// `hashes` maps region file names in `region_dir` to their hashes.
pub fn render_dimension(
    cfg: &Config,
    dim: Dimension,
    region_dir: &Path,
    hashes: &BTreeMap<String, String>,
    out: &Path,
    log: Log,
) -> Result<DimStats> {
    fs::create_dir_all(out)?;
    let journal_path = out.join("sources.json");
    let mut journal: Journal = read_json(&journal_path).ok().flatten().unwrap_or_default();
    if journal.renderer != RENDERER_ID {
        // Empty folder, or tiles drawn by something else: start clean.
        clear_dir(out)?;
        journal = Journal { renderer: RENDERER_ID.to_string(), regions: BTreeMap::new() };
    }

    let mut wanted: BTreeMap<&str, (i32, i32)> = BTreeMap::new();
    for name in hashes.keys() {
        if let Some(xz) = parse_region_name(name) {
            wanted.insert(name.as_str(), xz);
        }
    }

    // Tiles of regions that no longer exist
    let mut removed: BTreeSet<(i32, i32)> = BTreeSet::new();
    let stale: Vec<String> = journal.regions.keys().filter(|n| !wanted.contains_key(n.as_str())).cloned().collect();
    for name in stale {
        if let Some((x, y)) = parse_region_name(&name) {
            let _ = fs::remove_file(tile_path(out, 0, x, y));
            removed.insert((x, y));
        }
        journal.regions.remove(&name);
    }
    let wanted_xy: BTreeSet<(i32, i32)> = wanted.values().copied().collect();
    for (x, y) in list_level(out, 0)? {
        if !wanted_xy.contains(&(x, y)) {
            let _ = fs::remove_file(tile_path(out, 0, x, y));
            removed.insert((x, y));
        }
    }

    // What needs drawing
    let mut jobs: Vec<Job> = Vec::new();
    for (&name, &(rx, rz)) in &wanted {
        let hash = &hashes[name];
        let current = match journal.regions.get(name) {
            Some(v) if *v == format!("{hash}:empty") => true,
            Some(v) if v == hash => tile_path(out, 0, rx, rz).is_file(),
            _ => false,
        };
        if !current {
            jobs.push(Job { name: name.to_string(), hash: hash.clone(), rx, rz });
        }
    }

    let total = jobs.len();
    let mut stats = DimStats { reused: wanted.len() - total, ..Default::default() };
    let mut unknown: BTreeSet<String> = BTreeSet::new();
    let mut first_failure: Option<String> = None;
    let pool = build_pool(cfg)?;
    let nether = dim == Dimension::Nether;

    if total > 0 {
        log(&format!("{total} region(s) to draw, {} unchanged", stats.reused), 0.0);
        let started = Instant::now();
        fs::create_dir_all(out.join("zoom.0"))?;
        let (tx, rx) = mpsc::channel::<Outcome>();
        let jobs_ref = &jobs;
        let pool_ref = &pool;
        let mut journal_error: Option<Error> = None;

        std::thread::scope(|s| {
            s.spawn(move || {
                pool_ref.install(|| {
                    jobs_ref.par_iter().for_each_with(tx, |tx, job| {
                        // A bug while drawing one region must not take the whole app down.
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            render_job(job, region_dir, out, nether)
                        }))
                        .unwrap_or_else(|_| Outcome {
                            name: job.name.clone(),
                            hash: job.hash.clone(),
                            kind: Kind::Failed("internal error while drawing this region".into()),
                            unknown: BTreeSet::new(),
                            note: None,
                        });
                        let _ = tx.send(outcome);
                    });
                });
            });

            let mut done = 0usize;
            for o in rx {
                done += 1;
                unknown.extend(o.unknown.iter().cloned());
                if let Some(n) = &o.note {
                    log(&format!("{}: {n}", o.name), done as f32 / total as f32 * 0.85);
                }
                match o.kind {
                    Kind::Tile => {
                        stats.rendered += 1;
                        journal.regions.insert(o.name, o.hash);
                    }
                    Kind::Empty => {
                        stats.empty += 1;
                        journal.regions.insert(o.name, format!("{}:empty", o.hash));
                    }
                    Kind::Failed(why) => {
                        stats.failed += 1;
                        log(&format!("[warn] {} skipped: {why}", o.name), done as f32 / total as f32 * 0.85);
                        first_failure.get_or_insert(why);
                    }
                }
                if done % 10 == 0 || done == total {
                    let secs = started.elapsed().as_secs();
                    log(&format!("drew {done}/{total} regions ({secs}s)"), done as f32 / total as f32 * 0.85);
                }
                if done % 40 == 0 {
                    if let Err(e) = save_journal(&journal_path, &journal) {
                        journal_error.get_or_insert(e);
                    }
                }
            }
        });
        save_journal(&journal_path, &journal)?;
        if let Some(e) = journal_error {
            return Err(e);
        }
        if stats.failed == total {
            return Err(Error::Renderer(format!(
                "none of the {total} regions could be drawn: {}",
                first_failure.unwrap_or_default()
            )));
        }
        if !unknown.is_empty() {
            let list: Vec<&str> = unknown.iter().map(|s| s.as_str()).take(40).collect();
            log(&format!("[warn] blocks without a colour (drawn grey): {}", list.join(", ")), 0.86);
        }
    } else {
        log("every region is unchanged, reusing the previous tiles", 0.85);
        save_journal(&journal_path, &journal)?;
    }

    let level0 = list_level(out, 0)?;
    log(&format!("building zoom levels from {} tiles", level0.len()), 0.87);
    build_pyramid(&pool, out, &level0, &removed, log)?;
    log("tiles done", 1.0);
    Ok(stats)
}

fn parent_of((x, y): (i32, i32)) -> (i32, i32) {
    (x.div_euclid(2), y.div_euclid(2))
}

fn parent_stale(dim_dir: &Path, z: i32, (px, py): (i32, i32), cur: &BTreeSet<(i32, i32)>, forced: &BTreeSet<(i32, i32)>) -> bool {
    let pm = match fs::metadata(tile_path(dim_dir, z - 1, px, py)).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };
    for dy in 0..2 {
        for dx in 0..2 {
            let c = (px * 2 + dx, py * 2 + dy);
            if forced.contains(&c) {
                return true;
            }
            if cur.contains(&c) {
                match fs::metadata(tile_path(dim_dir, z, c.0, c.1)).and_then(|m| m.modified()) {
                    Ok(t) if t > pm => return true,
                    Ok(_) => {}
                    Err(_) => return true,
                }
            }
        }
    }
    false
}

/// Shrink a child tile to half size (2 x 2 average, weighted by opacity) into one quadrant.
fn downsample_into(child: &[u8], out: &mut [u8], ox: usize, oy: usize) {
    let w = TILE_SIZE as usize;
    let half = w / 2;
    for y in 0..half {
        for x in 0..half {
            let mut sum = [0u32; 3];
            let mut asum = 0u32;
            for (dx, dy) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
                let i = ((2 * y + dy) * w + 2 * x + dx) * 4;
                let a = child[i + 3] as u32;
                sum[0] += child[i] as u32 * a;
                sum[1] += child[i + 1] as u32 * a;
                sum[2] += child[i + 2] as u32 * a;
                asum += a;
            }
            if asum > 0 {
                let o = ((oy + y) * w + ox + x) * 4;
                out[o] = (sum[0] / asum) as u8;
                out[o + 1] = (sum[1] / asum) as u8;
                out[o + 2] = (sum[2] / asum) as u8;
                out[o + 3] = (asum / 4) as u8;
            }
        }
    }
}

fn make_parent(dim_dir: &Path, z: i32, (px, py): (i32, i32), cur: &BTreeSet<(i32, i32)>) -> Result<()> {
    let w = TILE_SIZE as usize;
    let half = w / 2;
    let mut buf = vec![0u8; w * w * 4];
    let mut any = false;
    for dy in 0..2usize {
        for dx in 0..2usize {
            let c = (px * 2 + dx as i32, py * 2 + dy as i32);
            if !cur.contains(&c) {
                continue;
            }
            let child = read_png(&tile_path(dim_dir, z, c.0, c.1))?;
            downsample_into(&child, &mut buf, dx * half, dy * half);
            any = true;
        }
    }
    let path = tile_path(dim_dir, z - 1, px, py);
    if any {
        write_png(&path, TILE_SIZE, TILE_SIZE, buf)
    } else {
        let _ = fs::remove_file(&path);
        Ok(())
    }
}

/// The tile grid is anchored at the world origin, so tiles on both sides of it never merge:
/// the top level can have up to 2 x 2 tiles (x and y in -1..=0). That is where we stop.
fn needs_more_levels(cur: &BTreeSet<(i32, i32)>) -> bool {
    cur.len() > 1 && cur.iter().any(|&(x, y)| x < -1 || x > 0 || y < -1 || y > 0)
}

/// Build the zoom-out levels. A tile is redrawn when it is missing, a tile below it is newer,
/// or a tile below it was deleted.
fn build_pyramid(
    pool: &rayon::ThreadPool,
    dim_dir: &Path,
    level0: &BTreeSet<(i32, i32)>,
    removed: &BTreeSet<(i32, i32)>,
    log: Log,
) -> Result<()> {
    let mut cur = level0.clone();
    let mut forced = removed.clone();
    let mut z = 0i32;

    while needs_more_levels(&cur) && z > MIN_ZOOM {
        let parents: BTreeSet<(i32, i32)> = cur.iter().chain(forced.iter()).map(|&c| parent_of(c)).collect();
        let next: BTreeSet<(i32, i32)> = cur.iter().map(|&c| parent_of(c)).collect();
        let todo: Vec<(i32, i32)> = parents.iter().copied().filter(|&p| parent_stale(dim_dir, z, p, &cur, &forced)).collect();
        if !todo.is_empty() {
            log(&format!("zoom level {}: drawing {} tile(s)", z - 1, todo.len()), 0.9);
            pool.install(|| todo.par_iter().try_for_each(|&p| make_parent(dim_dir, z, p, &cur)))?;
        }
        for (x, y) in list_level(dim_dir, z - 1)? {
            if !next.contains(&(x, y)) {
                let _ = fs::remove_file(tile_path(dim_dir, z - 1, x, y));
            }
        }
        forced = parents.difference(&next).copied().collect();
        cur = next;
        z -= 1;
    }

    // Levels below the top are leftovers of a map that used to be bigger.
    for e in fs::read_dir(dim_dir)? {
        let e = e?;
        if let Some(k) = e.file_name().to_str().and_then(|n| n.strip_prefix("zoom.")).and_then(|n| n.parse::<i32>().ok()) {
            if k < z {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(())
}

/// Join the tiles of one dimension into a single PNG, using the most detailed zoom level
/// whose result stays within `max_px` pixels per side.
pub fn stitch(dim_dir: &Path, out_png: &Path, max_px: u32, log: Log) -> Result<()> {
    let mut chosen: Option<(i32, BTreeSet<(i32, i32)>)> = None;
    let mut z = 0;
    loop {
        let set = list_level(dim_dir, z)?;
        if set.is_empty() {
            break;
        }
        let (w, h, _, _) = extent(&set);
        let fits = w <= max_px as u64 && h <= max_px as u64 && w * h <= MAX_STITCH_PIXELS;
        chosen = Some((z, set));
        if fits {
            break;
        }
        z -= 1;
    }
    let Some((z, set)) = chosen else {
        return Err(Error::Renderer("no tiles to stitch".into()));
    };
    let (w, h, minx, miny) = extent(&set);
    let (w, h) = (w as usize, h as usize);
    log(&format!("stitching zoom level {z}: {w} x {h} pixels"), 0.0);

    let bg = [24u8, 26, 31, 255];
    let mut buf = vec![0u8; w * h * 4];
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }
    let ts = TILE_SIZE as usize;
    for (n, &(x, y)) in set.iter().enumerate() {
        let tile = read_png(&tile_path(dim_dir, z, x, y))?;
        let ox = (x - minx) as usize * ts;
        let oy = (y - miny) as usize * ts;
        for row in 0..ts {
            for col in 0..ts {
                let s = (row * ts + col) * 4;
                let a = tile[s + 3] as u32;
                if a == 0 {
                    continue;
                }
                let d = ((oy + row) * w + ox + col) * 4;
                if a == 255 {
                    buf[d..d + 3].copy_from_slice(&tile[s..s + 3]);
                } else {
                    for k in 0..3 {
                        buf[d + k] = ((tile[s + k] as u32 * a + buf[d + k] as u32 * (255 - a)) / 255) as u8;
                    }
                }
            }
        }
        if n % 50 == 0 {
            log("stitching tiles", n as f32 / set.len() as f32 * 0.8);
        }
    }
    log("saving the stitched image", 0.9);
    write_png(out_png, w as u32, h as u32, buf)
}

/// `(width px, height px, min x, min y)` of a set of tiles.
fn extent(set: &BTreeSet<(i32, i32)>) -> (u64, u64, i32, i32) {
    let minx = set.iter().map(|t| t.0).min().unwrap_or(0);
    let maxx = set.iter().map(|t| t.0).max().unwrap_or(0);
    let miny = set.iter().map(|t| t.1).min().unwrap_or(0);
    let maxy = set.iter().map(|t| t.1).max().unwrap_or(0);
    (
        (maxx - minx + 1) as u64 * TILE_SIZE as u64,
        (maxy - miny + 1) as u64 * TILE_SIZE as u64,
        minx,
        miny,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(parse_region_name("r.-3.5.mca"), Some((-3, 5)));
        assert_eq!(parse_region_name("r.0.0.mca"), Some((0, 0)));
        assert_eq!(parse_region_name("r.0.0.mcr"), None);
        assert_eq!(parse_tile_file("t.-1.-2.png"), Some((-1, -2)));
        assert_eq!(parse_tile_file("t.1.2.png.tmp"), None);
        assert_eq!(parent_of((-1, -2)), (-1, -1));
        assert_eq!(parent_of((3, 4)), (1, 2));
    }

    #[test]
    fn downsample_averages_and_respects_alpha() {
        let w = TILE_SIZE as usize;
        let mut child = vec![0u8; w * w * 4];
        // top-left 2x2 block: two opaque pixels (red, blue) and two transparent ones
        let set = |c: &mut Vec<u8>, x: usize, y: usize, px: [u8; 4]| c[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&px);
        set(&mut child, 0, 0, [200, 0, 0, 255]);
        set(&mut child, 1, 0, [0, 0, 100, 255]);
        let mut out = vec![0u8; w * w * 4];
        downsample_into(&child, &mut out, 0, 0);
        assert_eq!(&out[0..4], &[100, 0, 50, 127]);
        // a fully transparent area stays transparent
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
    }
}
