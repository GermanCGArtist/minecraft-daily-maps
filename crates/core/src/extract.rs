use crate::{
    error::{Error, Result},
    hashing,
    model::Dimension,
};
use rayon::prelude::*;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Extract `zip_path` into `dest`. `progress` gets a fraction 0..1.
/// Entries with unsafe paths (zip-slip) are skipped.
pub fn extract_zip(zip_path: &Path, dest: &Path, progress: &dyn Fn(f32)) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    let total = archive.len().max(1);
    fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = File::create(&out)?;
            io::copy(&mut entry, &mut f)?;
        }
        if i % 25 == 0 {
            progress(i as f32 / total as f32);
        }
    }
    progress(1.0);
    Ok(())
}

/// Find the folder that contains `level.dat` (the zip may wrap the world in a folder).
pub fn find_world_root(dir: &Path) -> Result<PathBuf> {
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in WalkDir::new(dir).max_depth(5).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() && entry.file_name() == "level.dat" {
            if let Some(parent) = entry.path().parent() {
                let depth = entry.depth();
                if best.as_ref().map_or(true, |(d, _)| depth < *d) {
                    best = Some((depth, parent.to_path_buf()));
                }
            }
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| {
        Error::Invalid("No level.dat found in the zip. Is this a Minecraft Java world?".into())
    })
}

/// Region folder of a dimension inside a world folder, if it exists.
pub fn region_dir(world: &Path, dim: Dimension) -> Option<PathBuf> {
    dim.region_candidates()
        .iter()
        .map(|c| world.join(c))
        .find(|p| p.is_dir())
}

/// Hash of every `.mca` file in a folder, keyed by file name.
pub fn hash_regions(region_dir: &Path) -> Result<BTreeMap<String, String>> {
    let files: Vec<PathBuf> = fs::read_dir(region_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "mca"))
        .collect();
    let hashed: Result<Vec<(String, String)>> = files
        .par_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            Ok((name, hashing::hash_file(p)?))
        })
        .collect();
    Ok(hashed?.into_iter().collect())
}

/// Recursively make `dst` look like `src`. Files are hard-linked when the file system allows
/// it (no extra disk space, instant), otherwise copied. This is only safe for files that are
/// replaced by writing a new file and renaming it, never modified in place, which is how all
/// tiles are written.
pub fn link_or_copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if fs::hard_link(entry.path(), &target).is_err() {
                fs::copy(entry.path(), &target)?;
                // keep the modification time: the zoom levels compare them
                if let Ok(modified) = fs::metadata(entry.path()).and_then(|m| m.modified()) {
                    if let Ok(f) = File::options().write(true).open(&target) {
                        let _ = f.set_modified(modified);
                    }
                }
            }
        }
    }
    Ok(())
}
