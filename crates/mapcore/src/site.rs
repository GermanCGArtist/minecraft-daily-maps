//! Builds the static HTML: per-snapshot pages, the day-slider site,
//! `days.json` / `days.js` and `latest.json`.

use crate::diffcalc::{Change, ChangeAccumulator};
use crate::model::*;
use crate::store::{write_json, SnapRef, Store};
use crate::tilepath::TileKey;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;

const VIEWER_JS: &str = include_str!("../assets/viewer.js");
const VIEWER_CSS: &str = include_str!("../assets/viewer.css");
const INDEX_HTML: &str = include_str!("../assets/index.html");
const SNAPSHOT_HTML: &str = include_str!("../assets/snapshot.html");

/// Make a JSON string safe to embed in an inline <script>.
fn script_safe(json: &str) -> String {
    json.replace("</", "<\\/").replace("<!--", "<\\u0021--")
}

fn hhmm(id: &str) -> String {
    if id.len() == 4 {
        format!("{}:{}", &id[0..2], &id[2..4])
    } else {
        id.to_string()
    }
}

type ChangesJson = BTreeMap<String, BTreeMap<String, Vec<(i32, i32, String)>>>;

fn changes_of(diff: &Option<DiffFile>) -> ChangesJson {
    let mut out = ChangesJson::new();
    if let Some(d) = diff {
        for (dim, dd) in &d.dimensions {
            if !dd.tiles.is_empty() {
                out.insert(dim.clone(), dd.tiles.clone());
            }
        }
    }
    out
}

/// Net changes over all snapshots of a day (what the slider shows).
fn day_changes(store: &Store, refs: &[SnapRef]) -> ChangesJson {
    let mut acc: BTreeMap<String, ChangeAccumulator> = BTreeMap::new();
    for r in refs {
        if let Some(diff) = store.read_diff(r) {
            for (dim, dd) in diff.dimensions {
                let mut list: Vec<(TileKey, Change)> = Vec::new();
                for (zoom, tiles) in dd.tiles {
                    let z: i32 = match zoom.parse() {
                        Ok(z) => z,
                        Err(_) => continue,
                    };
                    for (x, y, kind) in tiles {
                        if let Some(c) = Change::from_code(&kind) {
                            list.push(((z, x, y), c));
                        }
                    }
                }
                acc.entry(dim).or_default().apply(&list);
            }
        }
    }
    let mut out = ChangesJson::new();
    for (dim, a) in acc {
        let mut per_zoom: BTreeMap<String, Vec<(i32, i32, String)>> = BTreeMap::new();
        for ((z, x, y), c) in a.into_vec() {
            per_zoom
                .entry(z.to_string())
                .or_default()
                .push((x, y, c.code().to_string()));
        }
        if !per_zoom.is_empty() {
            out.insert(dim, per_zoom);
        }
    }
    out
}

fn entry_json(meta: &Meta, base: &str, snapshots: &[String], changes: &ChangesJson) -> Value {
    json!({
        "date": meta.date,
        "time": meta.time,
        "local_datetime": meta.local_datetime,
        "utc_offset": meta.utc_offset,
        "base": base,
        "snapshots": snapshots,
        "dimensions": meta.dimensions,
        "changes": changes,
    })
}

fn refs_of_day(store: &Store, date: &str) -> Vec<SnapRef> {
    store
        .list_refs()
        .into_iter()
        .filter(|r| r.date == date)
        .collect()
}

/// Write `index.html` for one snapshot (self-contained: viewer inlined).
pub fn build_snapshot_page(store: &Store, r: &SnapRef) -> Result<()> {
    let meta = match store.read_meta(r) {
        Some(m) if !m.dimensions.is_empty() => m,
        _ => return Ok(()), // nothing rendered yet
    };
    let diff = store.read_diff(r);
    let day_refs = refs_of_day(store, &r.date);
    let day_ids: Vec<String> = day_refs.iter().map(|x| x.id.clone()).collect();
    let siblings: Vec<Value> = day_refs
        .iter()
        .filter(|x| store.read_meta(x).map(|m| !m.dimensions.is_empty()).unwrap_or(false))
        .map(|x| {
            json!({
                "id": x.id,
                "label": format!("{} {}", x.date, hhmm(&x.id)),
                "href": if x.id == r.id { "index.html".to_string() } else { format!("../{}/index.html", x.id) },
                "current": x.id == r.id,
            })
        })
        .collect();
    let title = format!("{} {}", r.date, hhmm(&r.id));
    let data = json!({
        "mode": "snapshot",
        "prefix": "",
        "title": format!("Minecraft map \u{00B7} {}", title),
        "entries": [entry_json(&meta, ".", &day_ids, &changes_of(&diff))],
        "siblings": siblings,
    });
    let html = SNAPSHOT_HTML
        .replace("{{TITLE}}", &format!("Minecraft map {}", title))
        .replace("{{CSS}}", VIEWER_CSS)
        .replace("{{DATA}}", &script_safe(&serde_json::to_string(&data)?))
        .replace("{{JS}}", VIEWER_JS);
    fs::write(store.snapshot_dir(&r.date, &r.id).join("index.html"), html)?;
    Ok(())
}

/// Rebuild all snapshot pages of one day and its `latest.json`.
pub fn rebuild_day(store: &Store, date: &str) -> Result<()> {
    let refs = refs_of_day(store, date);
    for r in &refs {
        build_snapshot_page(store, r)?;
    }
    let latest = refs
        .iter()
        .filter(|r| {
            store
                .read_meta(r)
                .map(|m| !m.dimensions.is_empty())
                .unwrap_or(false)
        })
        .last();
    match latest {
        Some(l) => write_json(
            &store.day_dir(date).join("latest.json"),
            &json!({ "snapshot": l.id }),
        )?,
        None => {
            let _ = fs::remove_file(store.day_dir(date).join("latest.json"));
        }
    }
    Ok(())
}

/// Rebuild `site/`: viewer assets, `days.json` and `days.js`.
pub fn rebuild_index(store: &Store) -> Result<()> {
    let site = store.site_dir();
    fs::create_dir_all(&site)?;
    fs::write(site.join("index.html"), INDEX_HTML)?;
    fs::write(site.join("viewer.js"), VIEWER_JS)?;
    fs::write(site.join("viewer.css"), VIEWER_CSS)?;

    let refs = store.list_refs();
    let mut dates: Vec<String> = refs.iter().map(|r| r.date.clone()).collect();
    dates.dedup();

    let mut entries: Vec<Value> = Vec::new();
    for date in dates {
        let day_refs: Vec<SnapRef> = refs.iter().filter(|r| r.date == date).cloned().collect();
        let complete: Vec<&SnapRef> = day_refs
            .iter()
            .filter(|r| {
                store
                    .read_meta(r)
                    .map(|m| !m.dimensions.is_empty())
                    .unwrap_or(false)
            })
            .collect();
        let latest = match complete.last() {
            Some(l) => (*l).clone(),
            None => continue,
        };
        let meta = match store.read_meta(&latest) {
            Some(m) => m,
            None => continue,
        };
        let ids: Vec<String> = complete.iter().map(|r| r.id.clone()).collect();
        let base = format!("days/{}/{}", latest.date, latest.id);
        entries.push(entry_json(
            &meta,
            &base,
            &ids,
            &day_changes(store, &day_refs),
        ));
    }

    let index = json!({
        "schema": SCHEMA_VERSION,
        "generated_at": crate::timeutil::now_rfc3339(),
        "entries": entries,
    });
    write_json(&site.join("days.json"), &index)?;

    let view = json!({
        "mode": "slider",
        "prefix": "../",
        "title": "Minecraft Daily Maps",
        "entries": index["entries"],
    });
    fs::write(
        site.join("days.js"),
        format!(
            "window.MAPVIEW = {};\n",
            script_safe(&serde_json::to_string(&view)?)
        ),
    )?;
    Ok(())
}

/// Rebuild everything (all day pages and the site).
pub fn rebuild_all(store: &Store) -> Result<()> {
    let mut dates: Vec<String> = store.list_refs().into_iter().map(|r| r.date).collect();
    dates.dedup();
    for d in dates {
        rebuild_day(store, &d)?;
    }
    rebuild_index(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_safe_escapes_closing_tags() {
        let s = script_safe("{\"a\":\"</script><!--\"}");
        assert!(!s.contains("</script>"));
        assert!(!s.contains("<!--"));
        // still valid JSON
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["a"], "</script><!--");
    }

    #[test]
    fn hhmm_format() {
        assert_eq!(hhmm("0005"), "00:05");
    }
}
