# Minecraft Daily Maps

A desktop app (Tauri 2 + Rust) that turns the world zip you download every day into a browsable,
diffable tiled map. Drop a zip in, get one folder per day, an HTML viewer per snapshot, and a
day slider across all days that can outline what changed.

Rendering is done by the [uNmINeD](https://unmined.net) CLI, which you install separately.

> **Status: first build.** The pipeline, viewer and UI follow [docs/design.md](docs/design.md), but this
> first version was written without being able to compile the Rust code or run the app. See
> [Verification status](#verification-status) before relying on it.

## How it works

1. Drop `world_2026-10-22_0005.zip` onto the window (or into `inbox/`). Date and time are read from the
   file name, otherwise from the file's modified time, and can be edited before you confirm.
2. The zip is hashed (duplicates are skipped), copied into `days/<date>/<HHMM>/zip/` and extracted.
3. For each selected dimension the app compares the `.mca` region files with the previous snapshot. If
   none changed, the previous tiles are reused. Otherwise `unmined-cli web render` runs and its tiles are
   normalised to `render/tiles/<dimension>/<zoom>/<x>/<y>.png`.
4. Every tile is hashed and compared with the previous snapshot: `diff.json` lists added, modified and
   removed tiles.
5. HTML is generated: a self-contained `index.html` per snapshot, and `site/index.html` with the day
   slider.

## Data folder

Chosen on first launch.

```
minecraft-maps/
├── inbox/                      drop zips here; they show up in the queue
├── days/2026-10-22/
│   ├── 0005/                   one folder per snapshot (HHMM, local time)
│   │   ├── zip/
│   │   ├── world/              extracted world (deleted after rendering unless "keep world")
│   │   ├── render/
│   │   │   ├── tiles/{overworld,nether,end}/{zoom}/{x}/{y}.png
│   │   │   ├── meta.json
│   │   │   ├── regions.json    size:sha256 per .mca file (change pre-check)
│   │   │   └── tilehashes.json sha256 per tile (exact diff)
│   │   ├── diff.json
│   │   └── index.html          open this to view the snapshot, works from disk
│   ├── 1537/
│   └── latest.json
├── site/                       index.html (day slider), days.json, days.js, viewer.js/css
└── config.json
```

Open `site/index.html` (or "Open map viewer" in the app) for the slider. The pages work straight from
disk; they embed their data in a script tag because browsers block `fetch()` of local JSON.
A snapshot folder can be zipped and shared on its own.

### Data formats

- `render/meta.json`: date, time, `local_datetime`, `utc_offset`, source zip name and `zip_sha256`, and per
  dimension the zoom range, tile bounds per zoom, tile count and whether tiles were reused.
- `diff.json`: `against` (previous snapshot or `null`) and per dimension `added/modified/removed` counts
  plus `tiles`: `{ "<zoom>": [[x, y, "a"|"m"|"r"], ...] }`. When the previous snapshot has no tiles for a
  dimension, it is marked `baseline: true` and nothing is reported as changed.
- `site/days.json`: one entry per day (its latest snapshot) with that day's combined changes. Added
  then removed again within a day cancels out.

## Using it

1. Install the uNmINeD CLI for your OS from <https://unmined.net> and extract it somewhere.
   On macOS you may need to clear the download quarantine flag
   (`xattr -dr com.apple.quarantine <folder>`) and make it executable.
2. Start the app, pick a data folder, then in Settings set the path to `unmined-cli`
   (one path is stored per OS) and the default dimensions.
3. Drop a zip. Watch the queue. Click **Open** on the snapshot or **Open map viewer**.

Settings: default dimensions, `--zoomin` (each level is 4x more tiles), keep or delete the extracted
world, hard-link unchanged tiles instead of copying (same volume only), auto-import from `inbox/`,
extra uNmINeD arguments.

Zips from Spigot/Paper servers that split dimensions into `world/`, `world_nether/` and
`world_the_end/` are recognised, as are vanilla worlds with `DIM-1/` and `DIM1/`.

## Development

Requirements: Rust (stable), Node 18+, and on Linux the
[Tauri system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm install            # installs the Tauri CLI
npm run dev            # run the app
npm run build          # installers for the current OS
cargo test -p mapcore  # pipeline tests (uses a fake unmined-cli on Unix)
npm run test:viewer    # viewer tests (Node, no dependencies)
python3 scripts/make_icons.py   # regenerate the placeholder icons
```

Layout: `crates/mapcore` is the whole pipeline and has no Tauri dependency. `src-tauri` is a thin shell
(job queue, inbox watcher, commands). `ui/` is the control panel. The viewer lives in
`crates/mapcore/assets/` and is embedded into the binary.

Pushing a tag like `v0.1.0` builds installers for macOS (Apple Silicon and Intel), Windows and Linux via
GitHub Actions and attaches them to a draft release. The builds are unsigned.

## Verification status

What has actually been run:

- `namefmt`, `tilepath` and `diffcalc` (file-name dates, tile path recognition, tile diffing and
  merging): 22 unit tests pass.
- The viewer's math and its DOM behaviour against a small DOM stub: 13 tests pass. It has **not** been
  opened in a real browser.
- Every Rust file parses (`rustfmt`).

What has not:

- **Nothing that depends on crates (`zip`, `serde`, `sha2`, `chrono`, `walkdir`, `tauri`) has been
  compiled**, because the environment this was written in could not reach crates.io. Expect a handful of
  small compile errors on the first `cargo build`; the CI workflow will show them on all three OSes.
- The end-to-end pipeline test (`crates/mapcore/tests/pipeline.rs`) exists but has not been run.
- The app has never been launched, so drag and drop, notifications and the UI are untested.

## Known gaps and assumptions

- **uNmINeD output layout.** The tile layout of `unmined-cli web render` could not be confirmed from its
  documentation. The importer therefore recognises tiles by shape (a `zoom.<n>` folder plus
  `tile.<x>.<y>.png`, or `z/x/y.png`) and fails with a clear message if it finds none. The command used
  is `web render --world --output --dimension --zoomin`; check these against `unmined-cli web render --help`
  for your version and use "Extra uNmINeD arguments" for anything else.
- **Tile geometry.** The viewer assumes tile `(x, y)` at zoom `z` covers
  `256 / 2^z` blocks per side with X east and Z south (zoom 0 = 1 pixel per block). If the coordinate
  readout under the cursor is off against the game, this is the first place to look (`viewer.js`).
- **Region pre-check is all-or-nothing per dimension.** If every `.mca` file is identical to the previous
  snapshot, the previous tiles are reused; if any changed, the dimension is rendered in full. Partial
  re-rendering would need the renderer's own incremental mode.
- **Stitched image** (`render/stitched/`, marked optional in the design) is not implemented.
  `unmined-cli image render --trim` would be the natural way to add it.
- Tile hashes are of the PNG bytes, so a renderer that embeds timestamps in its PNGs would report every
  tile as changed.
- Builds are unsigned: macOS Gatekeeper and Windows SmartScreen will warn.
- The icons are placeholders.
