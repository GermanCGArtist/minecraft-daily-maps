# Minecraft Daily Maps, v0.4.0

Desktop app (Rust + Tauri 2) that takes a Minecraft Java world zip per day, draws the map
itself (no other map tool needed), keeps every snapshot in its own self-contained folder, and
builds HTML viewers with a day slider, per-day pages, dimension tabs and a "show changes" overlay.

```
crates/core/   pipeline, no GUI: extract, hash, draw tiles, diff, build HTML (+ tests)
src-tauri/     Tauri shell: job queue, inbox watcher, commands
ui/            control panel (plain HTML/JS, no npm)
```

## Build and run (Windows / macOS / Linux)

Prerequisites: Rust (stable), and on Windows the WebView2 runtime (already part of Windows 11).
Linux also needs the usual Tauri system packages (webkit2gtk-4.1 etc.).

```
cargo install tauri-cli --version "^2" --locked     # once
cargo test -p mcmap-core                            # pipeline tests, no GUI needed
cargo tauri dev                                     # run the app
cargo tauri build                                   # installer / bundle
```

Run the commands from this folder (the one containing `Cargo.toml`).
For a macOS bundle, run `cargo tauri icon src-tauri/icons/icon.png` once to generate `icon.icns`
and add it to `bundle.icon` in `src-tauri/tauri.conf.json`.

## First launch

1. Choose a data folder. The app creates `inbox/`, `days/` and `site/` in it.
2. Settings: pick the default dimensions, render threads and, if you want it, the stitched image.
3. Drop a world zip on the window (or into `inbox/`). Check the date and time, confirm.
4. "Open map viewer" opens `site/index.html`; "Open" on a snapshot opens that snapshot alone.

## What gets written

```
<data>/days/2026-10-22/0005/{zip,world,render,diff.json,index.html,...}
<data>/days/2026-10-22/latest.json
<data>/site/{index.html, days.json, days-data.js, viewer.js, viewer.css}
<data>/config.json
```

Each snapshot folder is self-contained: copy it anywhere and open its `index.html`.
`site/index.html` expects the sibling `days/` folder.

## How the maps are drawn (v0.4.0)

The app reads the `.mca` region files itself (Minecraft 1.18+ chunk format, zlib/gzip/uncompressed
chunks) and draws every region as one 512 x 512 PNG, one pixel per block:

- For every column it takes the highest visible block and looks through air, plants, glass,
  torches and the like. Water is blended with the floor colour by depth.
- Colours come from the block (`crates/core/src/blocks.rs`) and the biome (grass, leaves and
  water colour). Light comes from the north-west: a column higher than its northern and western
  neighbour is brighter, a lower one darker.
- The Nether is drawn from below its bedrock roof, the End as it is (empty space stays transparent).
- Tiles: `zoom.0/t.X.Z.png` is the region `r.X.Z.mca`. `zoom.-1`, `zoom.-2`, ... are made from
  four tiles of the level below at half the resolution, until everything fits into 2 x 2 tiles.
- The optional stitched image (`render/stitched/<dimension>.png`) joins the tiles at the most
  detailed level that fits into the pixel limit from Settings.

## How incremental rendering works

Each dimension folder has a `sources.json`: which region file (by hash) each tile was drawn from.

- A new snapshot starts from the previous snapshot's tiles. They are hard links, so they take no
  extra disk space (the app falls back to copies if the disk does not allow links). Only regions
  whose file changed are drawn again; the zoom levels above them are rebuilt.
- A render that stops (crash, power loss, closed app) keeps all finished tiles; "Resume" draws only
  the missing regions.
- Tiles are hashed and compared with the previous snapshot to produce `diff.json` (level 0 = one
  entry per region).
- Snapshots made with older versions (uNmINeD tiles) still open. They are not compared with new
  snapshots, and "Re-render" draws them again with the new renderer.

## Known limits of the renderer

- No lighting or block-light, no cave view, no biome blending between neighbouring biomes
  (a biome edge is a hard edge), and no heightmap-accurate shading across region borders.
- Chunks in the pre-1.18 format and chunks stored with LZ4 compression are skipped and reported.
- Blocks without a colour rule are drawn grey and listed in the log ("blocks without a colour").
  Send me that list, or add a line in `blocks.rs`. After changing colours, bump `RENDERER_ID`
  in `crates/core/src/native.rs` so old tiles are not reused.

## Not implemented

Bedrock worlds, editing the date of an existing snapshot, editable date for inbox files
(they use the file name or modified time).

## Sharing one build folder between versions (optional)

Each version folder gets its own `target` folder (several GB). To compile dependencies only
once, create `.cargo\config.toml` in the version folder with:

```
[build]
target-dir = "../target"
```

or set `CARGO_TARGET_DIR` before running cargo. Delete old `target` folders to free space.
