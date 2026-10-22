//! Zip handling: hashing, safe extraction, locating the world folders.

use crate::model::Dimension;
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extract `zip_path` into `dest`. Entries that would escape `dest` are
/// rejected (zip-slip protection via `enclosed_name`).
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(zip_path).with_context(|| format!("opening {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("reading zip archive")?;
    fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => return Err(anyhow!("unsafe path in zip entry: {}", entry.name())),
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out =
            File::create(&out_path).with_context(|| format!("creating {}", out_path.display()))?;
        io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

fn has_mca(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(it) => it
            .flatten()
            .any(|e| e.path().extension().map(|x| x == "mca").unwrap_or(false)),
        Err(_) => false,
    }
}

/// Where each dimension's data lives inside an extracted zip.
///
/// Vanilla/singleplayer worlds keep all dimensions in one folder
/// (`region/`, `DIM-1/region/`, `DIM1/region/`). Bukkit-style servers
/// (Spigot/Paper) split them into `world/`, `world_nether/`, `world_the_end/`;
/// both shapes are found by searching a few levels deep.
#[derive(Debug, Clone, Default)]
pub struct WorldLayout {
    pub overworld: Option<PathBuf>,
    pub nether: Option<PathBuf>,
    pub end: Option<PathBuf>,
}

impl WorldLayout {
    /// The folder to pass to `unmined-cli --world` for a dimension.
    pub fn world_for(&self, dim: Dimension) -> Option<&Path> {
        match dim {
            Dimension::Overworld => self.overworld.as_deref(),
            Dimension::Nether => self.nether.as_deref(),
            Dimension::End => self.end.as_deref(),
        }
    }

    pub fn available(&self) -> Vec<Dimension> {
        Dimension::ALL
            .iter()
            .copied()
            .filter(|d| self.world_for(*d).is_some())
            .collect()
    }

    /// Region folder for a dimension (where the .mca files are).
    pub fn region_dir(&self, dim: Dimension) -> Option<PathBuf> {
        self.world_for(dim).map(|w| w.join(dim.region_subdir()))
    }
}

pub fn locate_worlds(extracted: &Path) -> WorldLayout {
    let mut layout = WorldLayout::default();
    // Shallowest match wins, so walk breadth-first (walkdir default is DFS,
    // therefore sort by depth).
    let mut dirs: Vec<(usize, PathBuf)> = WalkDir::new(extracted)
        .max_depth(4)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_dir())
        .map(|e| (e.depth(), e.path().to_path_buf()))
        .collect();
    dirs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    for (_, dir) in dirs {
        if layout.overworld.is_none() && has_mca(&dir.join(Dimension::Overworld.region_subdir())) {
            layout.overworld = Some(dir.clone());
        }
        if layout.nether.is_none() && has_mca(&dir.join(Dimension::Nether.region_subdir())) {
            layout.nether = Some(dir.clone());
        }
        if layout.end.is_none() && has_mca(&dir.join(Dimension::End.region_subdir())) {
            layout.end = Some(dir.clone());
        }
    }
    layout
}

/// Hash every `.mca` file of a region folder: `file name -> "size:sha256"`.
pub fn region_manifest(region_dir: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    let mut out = std::collections::BTreeMap::new();
    let rd = match fs::read_dir(region_dir) {
        Ok(r) => r,
        Err(_) => return Ok(out),
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x == "mca").unwrap_or(false) {
            let size = e.metadata()?.len();
            let hash = sha256_file(&p)?;
            out.insert(
                e.file_name().to_string_lossy().to_string(),
                format!("{}:{}", size, hash),
            );
        }
    }
    Ok(out)
}
