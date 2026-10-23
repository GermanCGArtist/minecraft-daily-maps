//! End-to-end tests: fake worlds (made by `mcmap_core::fixture`) go through the real pipeline.

use mcmap_core::{
    extract, fixture,
    model::{Config, DiffFile, Dimension},
    native,
    pipeline::{self, ImportRequest},
    store::read_json,
    Error, Store,
};
use std::{cell::RefCell, fs, io::Write, path::Path, time::SystemTime};

const GRASS: &str = "minecraft:grass_block";
const SAND: &str = "minecraft:sand";
const SNOW: &str = "minecraft:snow_block";

fn nether_region() -> Vec<u8> {
    let nbt = fixture::chunk_nbt(
        0..8,
        &|_x, y, _z| if y >= 123 { "minecraft:bedrock" } else if (40..=60).contains(&y) { "minecraft:air" } else { "minecraft:netherrack" },
        "minecraft:nether_wastes",
    );
    fixture::region_bytes(&[(0, 0, nbt)])
}

fn make_zip(path: &Path, regions: &[(&str, Vec<u8>)], nether: bool, extra: Option<&str>) {
    let mut zw = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zw.start_file("MyWorld/level.dat", opts).unwrap();
    zw.write_all(b"leveldata").unwrap();
    for (name, content) in regions {
        zw.start_file(format!("MyWorld/region/{name}"), opts).unwrap();
        zw.write_all(content).unwrap();
    }
    if nether {
        zw.start_file("MyWorld/DIM-1/region/r.0.0.mca", opts).unwrap();
        zw.write_all(&nether_region()).unwrap();
    }
    if let Some(e) = extra {
        zw.start_file("MyWorld/extra.txt", opts).unwrap();
        zw.write_all(e.as_bytes()).unwrap();
    }
    zw.finish().unwrap();
}

fn small_config() -> Config {
    let mut cfg = Config::default();
    cfg.render_threads = 2;
    cfg
}

fn mtime(p: &Path) -> SystemTime {
    fs::metadata(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).modified().unwrap()
}

fn req(zip: &Path, date: &str, time: &str) -> ImportRequest {
    ImportRequest {
        zip_path: zip.to_path_buf(),
        date: date.into(),
        time: time.into(),
        dimensions: vec![Dimension::Overworld],
        move_source: false,
    }
}

#[test]
fn full_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("maps")).unwrap();
    store.save_config(&small_config()).unwrap();
    let noop = |_: &str, _: &str, _: f32| {};
    let zips = tmp.path().join("zips");
    fs::create_dir_all(&zips).unwrap();

    // Day 1: two regions
    let a = zips.join("world_2026-10-21_0005.zip");
    make_zip(&a, &[("r.0.0.mca", fixture::flat_region(GRASS)), ("r.1.0.mca", fixture::flat_region(SAND))], true, None);
    let ma = pipeline::import(&store, &req(&a, "2026-10-21", "0005"), &noop).unwrap();
    assert_eq!(ma.dimensions, vec![Dimension::Overworld]);
    assert_eq!(ma.renderer, native::RENDERER_ID);
    assert_eq!(ma.tile_size, 512);
    let d1 = store.snapshot_dir("2026-10-21", "0005").join("render/tiles/overworld");
    for f in ["zoom.0/t.0.0.png", "zoom.0/t.1.0.png", "zoom.-1/t.0.0.png", "sources.json"] {
        assert!(d1.join(f).is_file(), "{f}");
    }
    // the tile is a real picture: grass colour inside the chunk, transparent outside
    let img = image::open(d1.join("zoom.0/t.0.0.png")).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (512, 512));
    assert_eq!(img.get_pixel(3, 3).0, [0x91, 0xBD, 0x59, 255]);
    assert_eq!(img.get_pixel(100, 100).0[3], 0);

    // Same zip again -> duplicate
    let dup = pipeline::import(&store, &req(&a, "2026-10-21", "0010"), &noop);
    assert!(matches!(dup, Err(Error::Duplicate(_))), "{dup:?}");
    assert!(!store.snapshot_dir("2026-10-21", "0010").exists(), "failed import must leave nothing behind");

    // Day 2: one region changed, one added
    let b = zips.join("world_2026-10-22_0005.zip");
    make_zip(
        &b,
        &[("r.0.0.mca", fixture::flat_region(GRASS)), ("r.1.0.mca", fixture::flat_region(SNOW)), ("r.2.0.mca", fixture::flat_region(GRASS))],
        true,
        None,
    );
    pipeline::import(&store, &req(&b, "2026-10-22", "0005"), &noop).unwrap();
    let d2 = store.snapshot_dir("2026-10-22", "0005").join("render/tiles/overworld");
    assert_eq!(mtime(&d1.join("zoom.0/t.0.0.png")), mtime(&d2.join("zoom.0/t.0.0.png")), "unchanged region keeps its tile");
    assert_ne!(fs::read(d1.join("zoom.0/t.1.0.png")).unwrap(), fs::read(d2.join("zoom.0/t.1.0.png")).unwrap());

    let diff: DiffFile = read_json(&store.snapshot_dir("2026-10-22", "0005").join("diff.json")).unwrap().unwrap();
    assert_eq!(diff.previous.as_deref(), Some("2026-10-21/0005"));
    let d = &diff.dimensions[&Dimension::Overworld];
    assert_eq!(d.z, 0);
    assert_eq!(d.changed, vec![(1, 0)]);
    assert_eq!(d.added, vec![(2, 0)]);
    assert!(d.removed.is_empty());

    // Day 1 is untouched by what happened on day 2 (tiles are shared, never modified in place)
    let img = image::open(d1.join("zoom.0/t.1.0.png")).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(3, 3).0, [219, 207, 163, 255], "sand");

    // Same day, identical regions, different zip bytes -> everything reused, empty diff
    let c = zips.join("world_2026-10-22_1537.zip");
    make_zip(
        &c,
        &[("r.0.0.mca", fixture::flat_region(GRASS)), ("r.1.0.mca", fixture::flat_region(SNOW)), ("r.2.0.mca", fixture::flat_region(GRASS))],
        true,
        Some("log"),
    );
    let lines = RefCell::new(Vec::<String>::new());
    pipeline::import(&store, &req(&c, "2026-10-22", "1537"), &|_: &str, d: &str, _: f32| lines.borrow_mut().push(d.to_string())).unwrap();
    assert!(lines.borrow().iter().any(|l| l.contains("unchanged")), "{:?}", lines.borrow());
    let d3 = store.snapshot_dir("2026-10-22", "1537").join("render/tiles/overworld");
    for f in ["zoom.0/t.0.0.png", "zoom.0/t.1.0.png", "zoom.0/t.2.0.png", "zoom.-1/t.0.0.png", "zoom.-1/t.1.0.png"] {
        assert_eq!(mtime(&d2.join(f)), mtime(&d3.join(f)), "{f} must not be drawn again");
    }
    let diff: DiffFile = read_json(&store.snapshot_dir("2026-10-22", "1537").join("diff.json")).unwrap().unwrap();
    let o = &diff.dimensions[&Dimension::Overworld];
    assert!(o.changed.is_empty() && o.added.is_empty() && o.removed.is_empty());

    // Rerender adds the Nether
    pipeline::rerender(&store, "2026-10-22", "1537", &[Dimension::Nether], &noop).unwrap();
    let snaps = store.list_snapshots().unwrap();
    assert_eq!(snaps.len(), 3);
    assert_eq!(snaps[2].dimensions, vec![Dimension::Overworld, Dimension::Nether]);
    assert!(!store.snapshot_dir("2026-10-22", "1537").join("world").exists(), "world deleted when keep_world=false");
    let nimg = image::open(store.snapshot_dir("2026-10-22", "1537").join("render/tiles/nether/zoom.0/t.0.0.png")).unwrap().to_rgba8();
    assert_eq!(nimg.get_pixel(4, 4).0, [98, 38, 38, 255], "netherrack floor, not the bedrock roof");

    // Generated pages
    let site = store.site_dir();
    for f in ["index.html", "viewer.js", "viewer.css", "days-data.js", "days.json"] {
        assert!(site.join(f).is_file(), "site/{f}");
    }
    let snap = store.snapshot_dir("2026-10-22", "0005");
    for f in ["index.html", "viewer.js", "viewer.css", "viewer-data.js", "diff-data.js", "diff.json", "render/tiles-data.js", "render/tiles.json", "render/meta.json"] {
        assert!(snap.join(f).is_file(), "snapshot/{f}");
    }
    let days = fs::read_to_string(site.join("days-data.js")).unwrap();
    assert!(days.contains("2026-10-21") && days.contains("2026-10-22") && days.contains("\"1537\""));
    let tiles_js = fs::read_to_string(snap.join("render/tiles-data.js")).unwrap();
    assert!(tiles_js.contains("zoom.0/t.1.0.png") && tiles_js.contains("zoom.-1/t.0.0.png") && tiles_js.contains("\"tile_size\":512"), "{tiles_js}");
    assert!(fs::read_to_string(store.days_dir().join("2026-10-22/latest.json")).unwrap().contains("1537"));

    // Deleting the middle snapshot re-links the diff chain and keeps the others intact
    pipeline::delete_snapshot(&store, "2026-10-22", "0005").unwrap();
    assert!(d3.join("zoom.0/t.1.0.png").is_file());
    image::open(d3.join("zoom.0/t.1.0.png")).unwrap();
    let diff: DiffFile = read_json(&store.snapshot_dir("2026-10-22", "1537").join("diff.json")).unwrap().unwrap();
    assert_eq!(diff.previous.as_deref(), Some("2026-10-21/0005"));
    let d = &diff.dimensions[&Dimension::Overworld];
    assert_eq!(d.changed, vec![(1, 0)]);
    assert_eq!(d.added, vec![(2, 0)]);
}

#[test]
fn rejects_non_world_zip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("maps")).unwrap();
    let z = tmp.path().join("x.zip");
    let mut zw = zip::ZipWriter::new(fs::File::create(&z).unwrap());
    zw.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
    zw.write_all(b"hi").unwrap();
    zw.finish().unwrap();
    let r = pipeline::import(&store, &req(&z, "2026-10-22", "0005"), &|_: &str, _: &str, _: f32| {});
    assert!(matches!(r, Err(Error::Invalid(_))), "{r:?}");
    assert!(store.list_snapshots().unwrap().is_empty());
}

#[test]
fn failed_render_is_kept_and_can_be_resumed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("maps")).unwrap();
    store.save_config(&small_config()).unwrap();

    // The only region holds a damaged chunk, so nothing can be drawn.
    let broken = fixture::region_bytes(&[(0, 0, vec![1, 2, 3])]);
    let zip = tmp.path().join("world_2026-10-22_0005.zip");
    make_zip(&zip, &[("r.0.0.mca", broken)], false, None);
    let request = req(&zip, "2026-10-22", "0005");
    let noop = |_: &str, _: &str, _: f32| {};

    // 1. The render fails: the error is reported, but the snapshot folder stays.
    let err = pipeline::import(&store, &request, &noop).unwrap_err();
    assert!(matches!(err, Error::Renderer(_)), "{err:?}");
    assert!(err.to_string().contains("could be drawn"), "{err}");
    assert!(store.list_snapshots().unwrap().is_empty(), "not a finished snapshot");
    let inc = store.list_incomplete().unwrap();
    assert_eq!(inc.len(), 1);
    assert!(inc[0].last_error.contains("could be drawn"));
    let dir = store.snapshot_dir("2026-10-22", "0005");
    assert!(dir.join("world").is_dir(), "extracted world kept for the resume");

    // 2. Importing the same zip again is refused with a pointer to Resume.
    let again = pipeline::import(&store, &request, &noop).unwrap_err();
    assert!(again.to_string().contains("unfinished"), "{again}");

    // 3. The cause is fixed (the region file is replaced); resume finishes the snapshot.
    fs::write(dir.join("world/MyWorld/region/r.0.0.mca"), fixture::flat_region(GRASS)).unwrap();
    let meta = pipeline::resume(&store, "2026-10-22", "0005", &noop).unwrap();
    assert_eq!(meta.dimensions, vec![Dimension::Overworld]);
    assert!(store.list_incomplete().unwrap().is_empty());
    assert_eq!(store.list_snapshots().unwrap().len(), 1);
    assert!(dir.join("index.html").is_file());
    assert!(dir.join("render/tiles/overworld/zoom.0/t.0.0.png").is_file());
    assert!(!dir.join("incomplete.json").exists());
    assert!(!dir.join("world").exists(), "world removed after success when keep_world=false");
}

#[test]
fn damaged_region_is_skipped_with_a_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("maps")).unwrap();
    store.save_config(&small_config()).unwrap();
    let zip = tmp.path().join("w.zip");
    make_zip(
        &zip,
        &[("r.0.0.mca", fixture::flat_region(GRASS)), ("r.1.0.mca", fixture::region_bytes(&[(0, 0, vec![1, 2, 3])]))],
        false,
        None,
    );
    let lines = RefCell::new(Vec::<String>::new());
    pipeline::import(&store, &req(&zip, "2026-10-22", "0005"), &|_: &str, d: &str, _: f32| lines.borrow_mut().push(d.to_string())).unwrap();
    assert!(lines.borrow().iter().any(|l| l.contains("[warn]") && l.contains("r.1.0.mca")), "{:?}", lines.borrow());
    let d = store.snapshot_dir("2026-10-22", "0005").join("render/tiles/overworld/zoom.0");
    assert!(d.join("t.0.0.png").is_file());
    assert!(!d.join("t.1.0.png").exists());
}

#[test]
fn native_renderer_is_incremental_and_builds_the_pyramid() {
    let tmp = tempfile::tempdir().unwrap();
    let region_dir = tmp.path().join("region");
    fs::create_dir_all(&region_dir).unwrap();
    let out = tmp.path().join("tiles");
    let cfg = small_config();
    let noop = |_: &str, _: f32| {};

    fs::write(region_dir.join("r.-1.0.mca"), fixture::flat_region(GRASS)).unwrap();
    fs::write(region_dir.join("r.0.0.mca"), fixture::flat_region(SAND)).unwrap();
    fs::write(region_dir.join("r.1.1.mca"), fixture::flat_region(GRASS)).unwrap();
    // an empty region file (never visited area) and a file that is not a region
    fs::write(region_dir.join("r.5.5.mca"), fixture::region_bytes(&[])).unwrap();
    fs::write(region_dir.join("notes.txt"), "x").unwrap();

    let hashes = extract::hash_regions(&region_dir).unwrap();
    let s = native::render_dimension(&cfg, Dimension::Overworld, &region_dir, &hashes, &out, &noop).unwrap();
    assert_eq!((s.rendered, s.empty, s.failed, s.reused), (3, 1, 0, 0));
    for f in ["zoom.0/t.-1.0.png", "zoom.0/t.0.0.png", "zoom.0/t.1.1.png", "zoom.-1/t.-1.0.png", "zoom.-1/t.0.0.png"] {
        assert!(out.join(f).is_file(), "{f}");
    }
    assert!(!out.join("zoom.0/t.5.5.png").exists(), "empty region has no tile");
    assert!(!out.join("zoom.-2").exists(), "the pyramid stops at 2 x 2 tiles around the origin");

    // A parent tile shows its four children at half size
    let p = image::open(out.join("zoom.-1/t.0.0.png")).unwrap().to_rgba8();
    assert_eq!(p.get_pixel(1, 1).0, [219, 207, 163, 255], "sand region (0,0) in the top-left quadrant");
    assert_eq!(p.get_pixel(256 + 1, 256 + 1).0, [0x91, 0xBD, 0x59, 255], "grass region (1,1) in the bottom-right quadrant");

    // Nothing changed: nothing is drawn, nothing is rewritten
    let before = mtime(&out.join("zoom.-1/t.0.0.png"));
    let s = native::render_dimension(&cfg, Dimension::Overworld, &region_dir, &hashes, &out, &noop).unwrap();
    assert_eq!((s.rendered, s.reused), (0, 4));
    assert_eq!(mtime(&out.join("zoom.-1/t.0.0.png")), before);

    // One region changes: its tile and the tiles above it are redrawn, the others are not
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let untouched = mtime(&out.join("zoom.0/t.-1.0.png"));
    fs::write(region_dir.join("r.1.1.mca"), fixture::flat_region(SNOW)).unwrap();
    let hashes = extract::hash_regions(&region_dir).unwrap();
    let s = native::render_dimension(&cfg, Dimension::Overworld, &region_dir, &hashes, &out, &noop).unwrap();
    assert_eq!((s.rendered, s.reused), (1, 3));
    assert_eq!(mtime(&out.join("zoom.0/t.-1.0.png")), untouched);
    let p = image::open(out.join("zoom.-1/t.0.0.png")).unwrap().to_rgba8();
    assert_eq!(p.get_pixel(256 + 1, 256 + 1).0, [248, 253, 253, 255], "snow now");
    assert!(mtime(&out.join("zoom.-1/t.0.0.png")) > before);

    // A region disappears: its tile goes, and so does the parent that only existed for it
    fs::remove_file(region_dir.join("r.-1.0.mca")).unwrap();
    let hashes = extract::hash_regions(&region_dir).unwrap();
    native::render_dimension(&cfg, Dimension::Overworld, &region_dir, &hashes, &out, &noop).unwrap();
    assert!(!out.join("zoom.0/t.-1.0.png").exists());
    assert!(!out.join("zoom.-1/t.-1.0.png").exists());
    assert!(out.join("zoom.-1/t.0.0.png").is_file());

    // The stitched image
    let png = tmp.path().join("stitched.png");
    native::stitch(&out, &png, 16384, &|_, _| {}).unwrap();
    let img = image::open(&png).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (1024, 1024), "tiles (0,0) and (1,1) at the most detailed level");
    assert_eq!(img.get_pixel(3, 3).0, [219, 207, 163, 255], "sand region at the top left");
    assert_eq!(img.get_pixel(512 + 3, 512 + 3).0, [248, 253, 253, 255], "snow region at the bottom right");
    assert_eq!(img.get_pixel(700, 100).0, [24, 26, 31, 255], "no region there: background");

    // A small limit picks a coarser zoom level
    let small = tmp.path().join("small.png");
    native::stitch(&out, &small, 600, &|_, _| {}).unwrap();
    assert_eq!(image::open(&small).unwrap().to_rgba8().dimensions(), (512, 512));
}
