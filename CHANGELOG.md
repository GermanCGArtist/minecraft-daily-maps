# Changelog

## v0.4.0
- **No more uNmINeD.** The app now reads the region files itself and draws the tiles:
  one 512 px PNG per region (1 pixel per block), plus zoom-out levels and an optional stitched
  image. This avoids the uNmINeD `GetJpegEncoder` crash completely.
- Block and biome colours, relief shading from the north-west, water drawn by depth,
  Nether drawn below its roof. Unknown blocks are drawn grey and listed in the log.
- Tiles of unchanged regions are reused from the previous snapshot as hard links (no extra disk
  space), only changed regions are drawn. A stopped render keeps every finished tile.
- Rendering runs on several threads (Settings > Render threads, default all cores but one).
- Settings: the uNmINeD path, zoom and extra arguments are gone; new: render threads and the
  size limit of the stitched image.
- Old snapshots (uNmINeD) still open; they are not diffed against new snapshots.
- Needs no uNmINeD download and no network.

## v0.3.0
- A failed render no longer deletes the snapshot folder. The partial tiles are kept and listed
  under "Unfinished imports" with Resume / Folder / Discard. Resume continues the uNmINeD render
  where it stopped (uNmINeD only renders what is missing).
- Importing the same zip again while an unfinished import exists tells you to resume or discard it.
- Known upstream problem: uNmINeD 0.20.11-dev can crash with `InvalidOperation_NoValue` in
  `GetJpegEncoder` while writing tiles (reported by other users, no fix known yet).

## v0.2.0
- Hashing: BLAKE3 (multi-threaded for the zip), dependencies optimised in debug builds.
- Live log panel in the app, plus `logs/app.log` in the data folder. Renderer stderr is shown.
- `cargo run` works without the Tauri CLI (`custom-protocol` on by default).
- UI no longer crashes if the log panel cannot be created; version shown in the title and log.
- Default extra zoom levels: 0.
- Optional: share one build folder between version folders to save disk space (see README).

## v0.1.0
- First version: pipeline, viewer, control panel.
