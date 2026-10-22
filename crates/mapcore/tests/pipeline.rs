//! End-to-end test of the import pipeline using a fake `unmined-cli`
//! shell script (Unix only). It writes one tile per region file so that
//! changing a region file changes exactly one tile.

#![cfg(unix)]

use mapcore::model::{Config, Dimension};
use mapcore::pipeline::{self, ImportOutcome, ImportRequest};
use mapcore::store::{SnapRef, Store};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const FAKE_UNMINED: &str = r#"#!/bin/sh
for a in "$@"; do
  case "$a" in
    --world=*) W="${a#--world=}";;
    --output=*) O="${a#--output=}";;
    --dimension=*) D="${a#--dimension=}";;
  esac
done
case "$D" in
  nether) R="$W/DIM-1/region";;
  end) R="$W/DIM1/region";;
  *) R="$W/region";;
esac
mkdir -p "$O/tiles/zoom.0"
for f in "$R"/r.*.0.mca; do
  n=$(basename "$f" | sed 's/r\.\([0-9-]*\)\.0\.mca/\1/')
  cp "$f" "$O/tiles/zoom.0/tile.$n.0.png"
done
echo "<html></html>" > "$O/unmined.index.html"
"#;

fn make_zip(path: &Path, files: &[(&str, &str)]) {
    let f = fs::File::create(path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    for (name, content) in files {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(content.as_bytes()).unwrap();
    }
    zw.finish().unwrap();
}

fn setup() -> (tempfile::TempDir, Store, Config) {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("fake-unmined.sh");
    fs::write(&script, FAKE_UNMINED).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let store = Store::new(tmp.path().join("minecraft-maps"));
    store.ensure_layout().unwrap();
    let mut cfg = Config::default();
    cfg.unmined_paths
        .set_current(script.to_string_lossy().to_string());
    cfg.zoom_in = 0;
    (tmp, store, cfg)
}

fn req(zip: PathBuf, date: &str, time: &str) -> ImportRequest {
    ImportRequest {
        zip_path: zip,
        date: date.to_string(),
        time: time.to_string(),
        dimensions: vec![Dimension::Overworld],
        move_zip: false,
    }
}

fn quiet(_: &str, _: &str, _: &str) {}

#[test]
fn import_diff_reuse_duplicate_and_delete() {
    let (tmp, store, cfg) = setup();

    // Snapshot 1: two regions.
    let z1 = tmp.path().join("world_2026-10-22_0005.zip");
    make_zip(
        &z1,
        &[
            ("world/level.dat", "x"),
            ("world/region/r.0.0.mca", "AAAA"),
            ("world/region/r.1.0.mca", "BBBB"),
        ],
    );
    let out =
        pipeline::import(&cfg, &store, &req(z1.clone(), "2026-10-22", "0005"), &quiet).unwrap();
    assert_eq!(
        out,
        ImportOutcome::Imported(SnapRef {
            date: "2026-10-22".into(),
            id: "0005".into()
        })
    );

    let snap1 = store.snapshot_dir("2026-10-22", "0005");
    assert!(snap1.join("render/tiles/overworld/0/0/0.png").exists());
    assert!(snap1.join("render/tiles/overworld/0/1/0.png").exists());
    assert!(snap1.join("render/meta.json").exists());
    assert!(snap1.join("diff.json").exists());
    assert!(snap1.join("index.html").exists());
    assert!(
        !snap1.join("world").exists(),
        "extracted world is removed by default"
    );
    assert!(store.site_dir().join("days.json").exists());
    assert!(store.site_dir().join("days.js").exists());

    let d1 = store
        .read_diff(&SnapRef {
            date: "2026-10-22".into(),
            id: "0005".into(),
        })
        .unwrap();
    assert!(d1.against.is_none());
    assert!(d1.dimensions["overworld"].baseline);

    // Snapshot 2, same day: region 0 changed.
    let z2 = tmp.path().join("world_2026-10-22_1537.zip");
    make_zip(
        &z2,
        &[
            ("world/level.dat", "x"),
            ("world/region/r.0.0.mca", "AAAB"),
            ("world/region/r.1.0.mca", "BBBB"),
        ],
    );
    pipeline::import(&cfg, &store, &req(z2.clone(), "2026-10-22", "1537"), &quiet).unwrap();
    let r2 = SnapRef {
        date: "2026-10-22".into(),
        id: "1537".into(),
    };
    let d2 = store.read_diff(&r2).unwrap();
    assert_eq!(d2.against.as_ref().unwrap().time, "0005");
    let ow = &d2.dimensions["overworld"];
    assert_eq!((ow.added, ow.modified, ow.removed), (0, 1, 0));
    assert_eq!(ow.tiles["0"], vec![(0, 0, "m".to_string())]);

    // Same zip again: detected as duplicate.
    let again =
        pipeline::import(&cfg, &store, &req(z2.clone(), "2026-10-23", "0005"), &quiet).unwrap();
    assert_eq!(again, ImportOutcome::Duplicate(r2.clone()));
    assert!(!store.snapshot_dir("2026-10-23", "0005").exists());

    // Next day: identical regions, different zip bytes -> tiles are reused.
    let z3 = tmp.path().join("world_2026-10-23_0005.zip");
    make_zip(
        &z3,
        &[
            ("world/level.dat", "different bytes"),
            ("world/region/r.0.0.mca", "AAAB"),
            ("world/region/r.1.0.mca", "BBBB"),
        ],
    );
    pipeline::import(&cfg, &store, &req(z3, "2026-10-23", "0005"), &quiet).unwrap();
    let r3 = SnapRef {
        date: "2026-10-23".into(),
        id: "0005".into(),
    };
    let m3 = store.read_meta(&r3).unwrap();
    assert!(m3.dimensions["overworld"].reused_previous);
    let d3 = store.read_diff(&r3).unwrap();
    assert!(d3.dimensions["overworld"].tiles.is_empty());

    // days.json: one entry per day, the latest snapshot of 2026-10-22 is 1537
    // and carries the day's combined changes.
    let days: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(store.site_dir().join("days.json")).unwrap())
            .unwrap();
    let entries = days["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["time"], "1537");
    assert_eq!(entries[0]["changes"]["overworld"]["0"][0][2], "m");
    let latest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(store.day_dir("2026-10-22").join("latest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(latest["snapshot"], "1537");

    // The page of the first snapshot lists both snapshots of its day.
    let html = fs::read_to_string(snap1.join("index.html")).unwrap();
    assert!(html.contains("../1537/index.html"));

    // Deleting the middle snapshot re-bases the next diff on the first one.
    pipeline::delete_snapshot(&store, &r2).unwrap();
    assert!(!store.snapshot_dir("2026-10-22", "1537").exists());
    let d3b = store.read_diff(&r3).unwrap();
    assert_eq!(d3b.against.as_ref().unwrap().time, "0005");
    assert_eq!(d3b.dimensions["overworld"].modified, 1);
}

#[test]
fn rerender_extracts_the_zip_again_when_world_was_deleted() {
    let (tmp, store, cfg) = setup();
    let z = tmp.path().join("w_2026-10-22_0005.zip");
    make_zip(&z, &[("level.dat", "x"), ("region/r.0.0.mca", "AAAA")]);
    pipeline::import(&cfg, &store, &req(z, "2026-10-22", "0005"), &quiet).unwrap();
    let r = SnapRef {
        date: "2026-10-22".into(),
        id: "0005".into(),
    };
    fs::remove_file(
        store
            .snapshot_dir(&r.date, &r.id)
            .join("render/tiles/overworld/0/0/0.png"),
    )
    .unwrap();
    pipeline::rerender(&cfg, &store, &r, &[Dimension::Overworld], &quiet).unwrap();
    assert!(store
        .snapshot_dir(&r.date, &r.id)
        .join("render/tiles/overworld/0/0/0.png")
        .exists());
}

#[test]
fn zip_without_regions_is_rejected() {
    let (tmp, store, cfg) = setup();
    let z = tmp.path().join("w_2026-10-22_0005.zip");
    make_zip(&z, &[("readme.txt", "hello")]);
    let err = pipeline::import(&cfg, &store, &req(z, "2026-10-22", "0005"), &quiet).unwrap_err();
    assert!(err.to_string().contains("region"));
}

#[test]
fn bukkit_style_split_worlds_are_found() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("x");
    fs::create_dir_all(root.join("world/region")).unwrap();
    fs::write(root.join("world/region/r.0.0.mca"), "a").unwrap();
    fs::create_dir_all(root.join("world_nether/DIM-1/region")).unwrap();
    fs::write(root.join("world_nether/DIM-1/region/r.0.0.mca"), "a").unwrap();
    let layout = mapcore::ingest::locate_worlds(&root);
    assert!(layout.overworld.as_ref().unwrap().ends_with("world"));
    assert!(layout.nether.as_ref().unwrap().ends_with("world_nether"));
    assert!(layout.end.is_none());
}
