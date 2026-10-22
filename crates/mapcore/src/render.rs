//! Calls the uNmINeD CLI and turns its output into the normalised tile layout
//! `tiles/<dimension>/<zoom>/<x>/<y>.png`.

use crate::model::*;
use crate::tilepath::{self, TileKey};
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

/// Build the argument list for `unmined-cli web render`.
pub fn unmined_args(cfg: &Config, world: &Path, out: &Path, dim: Dimension) -> Vec<String> {
    let mut args = vec![
        "web".to_string(),
        "render".to_string(),
        format!("--world={}", world.display()),
        format!("--output={}", out.display()),
        format!("--dimension={}", dim.key()),
        format!("--zoomin={}", cfg.zoom_in),
    ];
    args.extend(cfg.extra_unmined_args.iter().cloned());
    args
}

/// Run uNmINeD for one dimension. `out` must be an empty (or new) folder.
pub fn run_unmined(cfg: &Config, world: &Path, out: &Path, dim: Dimension) -> Result<()> {
    let exe = cfg.unmined_paths.current().trim();
    if exe.is_empty() {
        bail!("The uNmINeD CLI path is not set for this operating system. Set it in Settings.");
    }
    if !Path::new(exe).exists() {
        bail!(
            "uNmINeD CLI not found at '{}'. Check the path in Settings.",
            exe
        );
    }
    fs::create_dir_all(out)?;
    let output = Command::new(exe)
        .args(unmined_args(cfg, world, out, dim))
        .output()
        .with_context(|| format!("starting {}", exe))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut msg = format!("{}\n{}", stdout.trim(), stderr.trim());
        if msg.chars().count() > 2000 {
            let start = msg
                .char_indices()
                .rev()
                .nth(1999)
                .map(|(i, _)| i)
                .unwrap_or(0);
            msg = msg[start..].to_string();
        }
        return Err(anyhow!(
            "uNmINeD failed for {} ({}): {}",
            dim.key(),
            output.status,
            msg.trim()
        ));
    }
    Ok(())
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)
        .with_context(|| format!("copying {} -> {}", from.display(), to.display()))?;
    fs::remove_file(from)?;
    Ok(())
}

/// Move every recognised tile from the renderer's raw output into
/// `dest_dim_dir/<zoom>/<x>/<y>.png`. Returns the tile keys found.
pub fn normalise_tiles(raw: &Path, dest_dim_dir: &Path) -> Result<Vec<TileKey>> {
    if dest_dim_dir.exists() {
        fs::remove_dir_all(dest_dim_dir)?;
    }
    fs::create_dir_all(dest_dim_dir)?;
    let mut keys = Vec::new();
    // Collect first: moving files while walking the tree is asking for trouble.
    let files: Vec<PathBuf> = WalkDir::new(raw)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();
    for path in files {
        let rel = match path.strip_prefix(raw) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        if let Some(key) = tilepath::parse_tile_path(&rel) {
            let dest = dest_dim_dir.join(tilepath::normalised_rel(key));
            move_file(&path, &dest)?;
            keys.push(key);
        }
    }
    if keys.is_empty() {
        bail!(
            "uNmINeD finished but no tiles were recognised in {}. The tile layout may differ from what this tool expects.",
            raw.display()
        );
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

fn sha256_bytes_file(p: &Path) -> Result<String> {
    let data = fs::read(p).with_context(|| format!("reading {}", p.display()))?;
    let mut h = Sha256::new();
    h.update(&data);
    Ok(format!("{:x}", h.finalize()))
}

/// Hash every tile of a normalised dimension folder.
pub fn hash_tiles(dim_dir: &Path) -> Result<HashMap<TileKey, String>> {
    let mut out = HashMap::new();
    for entry in WalkDir::new(dim_dir).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = match entry.path().strip_prefix(dim_dir) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        if let Some(key) = tilepath::parse_tile_path(&rel) {
            out.insert(key, sha256_bytes_file(entry.path())?);
        }
    }
    Ok(out)
}

/// Copy (or hard-link) a normalised tile folder, used when a dimension is
/// unchanged since the previous snapshot.
pub fn copy_tiles(src: &Path, dst: &Path, hard_link: bool) -> Result<usize> {
    if dst.exists() {
        fs::remove_dir_all(dst)?;
    }
    let mut n = 0;
    for entry in WalkDir::new(src).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(src)?;
        let to: PathBuf = dst.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        let linked = hard_link && fs::hard_link(entry.path(), &to).is_ok();
        if !linked {
            fs::copy(entry.path(), &to)?;
        }
        n += 1;
    }
    Ok(n)
}

/// Summarise a set of tile keys as dimension metadata.
pub fn dimension_meta(
    keys: &[TileKey],
    region_count: usize,
    reused_previous: bool,
    zoom_in: u8,
) -> DimensionMeta {
    let per_zoom = tilepath::bounds_per_zoom(keys.iter());
    let mut bounds = std::collections::BTreeMap::new();
    for (z, b) in &per_zoom {
        bounds.insert(
            z.to_string(),
            Bounds {
                min_x: b.min_x,
                max_x: b.max_x,
                min_y: b.min_y,
                max_y: b.max_y,
            },
        );
    }
    DimensionMeta {
        tile_size: TILE_SIZE,
        zoom_min: per_zoom.first().map(|(z, _)| *z).unwrap_or(0),
        zoom_max: per_zoom.last().map(|(z, _)| *z).unwrap_or(0),
        bounds,
        tile_count: keys.len(),
        region_count,
        reused_previous,
        zoom_in,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_contain_required_flags() {
        let mut cfg = Config::default();
        cfg.zoom_in = 2;
        cfg.extra_unmined_args = vec!["--shadows=true".into()];
        let a = unmined_args(&cfg, Path::new("/w"), Path::new("/o"), Dimension::Nether);
        assert_eq!(&a[0..2], &["web".to_string(), "render".to_string()]);
        assert!(a.contains(&"--dimension=nether".to_string()));
        assert!(a.contains(&"--zoomin=2".to_string()));
        assert!(a.contains(&"--shadows=true".to_string()));
    }

    #[test]
    fn meta_from_keys() {
        let keys = vec![(0, 0, 0), (0, 2, 3), (1, 5, 5)];
        let m = dimension_meta(&keys, 4, false, 1);
        assert_eq!(m.zoom_min, 0);
        assert_eq!(m.zoom_max, 1);
        assert_eq!(m.tile_count, 3);
        assert_eq!(m.bounds["0"].max_y, 3);
    }
}
