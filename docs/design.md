# Minecraft Daily Mapping Tool: Agreed Design

## Stack
- Rust backend with Tauri 2 as the desktop shell
- Web UI (HTML/JS) inside the Tauri window
- uNmINeD CLI as the renderer, called as a subprocess
- Cross-platform: macOS and Windows are the primary targets, Linux should work too. Use platform-independent paths and a per-OS uNmINeD binary path in settings.

## Input
- Drop a world zip onto the window (real drag-and-drop), or into the `inbox/` folder, which also appears in the queue
- Date and time come from the filename (e.g. `world_2026-10-22_0005.zip`), otherwise from the zip's modified time. They are auto-filled and editable before confirming.
- Local time is stored, with the UTC offset in `meta.json`
- Duplicate zips are detected by hash and skipped

## Dimension selection
- Which dimensions to render (Overworld / Nether / End) is selectable
- Default selection is set in Settings, and it can be overridden per import and per re-render
- Only the selected dimensions get tiles and diffs. The viewer shows tabs only for dimensions that exist in a snapshot.

## Folder structure
```
minecraft-maps/
├── inbox/
├── days/
│   └── 2026-10-22/
│       ├── 0005/                  (one folder per snapshot, HHMM)
│       │   ├── zip/
│       │   ├── world/             (extracted, deletable)
│       │   ├── render/
│       │   │   ├── tiles/{overworld,nether,end}/{zoom}/{x}/{y}.png
│       │   │   ├── stitched/      (optional)
│       │   │   └── meta.json
│       │   ├── diff.json          (per dimension)
│       │   └── index.html
│       ├── 1537/
│       └── latest.json
├── site/                          (index.html, days.json, viewer.js/css)
└── config.json
```

## Multiple snapshots per day
- Each snapshot gets its own time folder, so nothing is overwritten
- The main slider uses the latest snapshot of each day
- Each per-day page has a snapshot dropdown

## Change detection
1. Cheap pre-check: compare `.mca` region files by size and hash, and skip rendering unchanged regions
2. Exact check: hash every rendered tile and compare with the previous snapshot, producing `diff.json`
- Each snapshot is compared with the previous snapshot in time. The main slider combines a day's diffs.

## Self-contained days
Each snapshot folder has its own tiles and `index.html`, so it can be shared on its own. Unchanged tiles are copied or linked in.

## Viewer UI
- Pan and zoom map, day slider with the date, dimension tabs
- "Show changes" toggle that outlines changed tiles
- Coordinates under the cursor
- Per-day pages are the same UI without the slider

## Control panel (desktop app)
- Drop zone with editable date/time and dimension checkboxes
- Queue with progress per step (extract, render per dimension, diff, build HTML)
- Snapshot list grouped by day: Open, Re-render, Delete
- Open map viewer, Open folder, OS notifications
- Settings: uNmINeD path, default dimensions, zoom levels, keep or delete the extracted world
- Data folder is chosen on first launch

## Build order
1. Data formats (`meta.json`, `diff.json`, `days.json`) and the viewer
2. Rust pipeline: extract, hash, render, diff, build HTML
3. Job queue and Tauri commands
4. Control panel UI
5. Packaging for macOS and Windows

## Open questions
- Packaging details: how to bundle or locate uNmINeD per OS (it ships separate builds for Windows, macOS and Linux)
- Whether tiles are copied or hard-linked into each day (hard links need the same volume and are less portable across systems)
