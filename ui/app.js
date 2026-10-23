/* Control panel. Talks to the Rust side through window.__TAURI__ (withGlobalTauri). */
"use strict";

const UI_VERSION = "0.4.0";

if (!window.__TAURI__) {
  document.body.textContent = "The Tauri API is not available. This page must run inside the app (check withGlobalTauri in tauri.conf.json).";
  throw new Error("window.__TAURI__ missing");
}
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialog = window.__TAURI__.dialog;

const DIMS = [["overworld", "Overworld"], ["nether", "Nether"], ["end", "End"]];
const STEPS = [["extract", "Extract"], ["render", "Render"], ["html", "Diff + pages"]];

let st = { data_dir: null, config: null, snapshots: [], jobs: [], os: "" };
let pending = [];            // drops waiting for confirmation: {path, date, time, dims:Set, source}
let rerenderOpen = null;     // "date/time" whose re-render picker is open
let deleteArmed = null;      // "date/time" waiting for a second click
let rerenderDims = new Set();

/* ---------- tiny DOM helper (always textContent, never innerHTML: file names are untrusted) ---------- */
function h(tag, props, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, "");
    else if (v !== false && v != null) el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) {
    if (kid == null || kid === false) continue;
    el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

function toast(msg, bad) {
  const t = document.getElementById("toast");
  t.textContent = msg; t.className = bad ? "bad" : ""; t.hidden = false;
  clearTimeout(toast.t);
  toast.t = setTimeout(() => { t.hidden = true; }, bad ? 7000 : 3000);
}

async function call(cmd, args) {
  try { return await invoke(cmd, args); }
  catch (e) { toast(String(e), true); throw e; }
}

function fmtTime(t) { return t.slice(0, 2) + ":" + t.slice(2); }
function dimLabel(d) { return (DIMS.find((x) => x[0] === d) || [d, d])[1]; }

/* ---------- rendering ---------- */
function render() {
  const root = document.getElementById("root");
  if (!root) { document.body.textContent = "Broken page: #root is missing."; return; }
  root.replaceChildren();
  if (!st.data_dir) { root.append(setupView()); return; }
  root.append(topBar(), dropZone(), pendingView(), queueView(), incompleteView(), snapshotsView());
}

function setupView() {
  return h("div", { class: "card setup" },
    h("h1", {}, "Minecraft Daily Maps"),
    h("p", {}, "Choose a folder where all days, tiles and viewers will be stored. You can pick an empty folder; the app creates inbox, days and site inside it."),
    h("button", { class: "primary", onclick: chooseDataFolder }, "Choose data folder…"));
}

async function chooseDataFolder() {
  const dir = await dialog.open({ directory: true, title: "Choose the data folder" });
  if (!dir) return;
  st = await call("set_data_dir", { path: dir });
  render();
}

function topBar() {
  return h("div", { class: "top" },
    h("h1", {}, "Minecraft Daily Maps ", h("small", { class: "muted" }, "v" + UI_VERSION)),
    h("button", { class: "primary", onclick: () => call("open_target", { kind: "site" }) }, "Open map viewer"),
    h("button", { onclick: () => call("open_target", { kind: "data_folder" }) }, "Open folder"),
    h("button", { onclick: openSettings }, "Settings"));
}

function dropZone() {
  return h("div", { class: "drop", id: "drop", onclick: browseZips },
    h("div", {}, h("b", {}, "Drop a world zip here"), " or click to browse"),
    h("small", {}, `Zips placed in ${st.data_dir}${st.os === "windows" ? "\\" : "/"}inbox are picked up automatically.`));
}

async function browseZips() {
  const picked = await dialog.open({ multiple: true, filters: [{ name: "World zip", extensions: ["zip"] }] });
  if (!picked) return;
  await addDrops(Array.isArray(picked) ? picked : [picked]);
}

async function addDrops(paths) {
  const zips = paths.filter((p) => /\.zip$/i.test(p));
  if (zips.length < paths.length) toast("Only .zip files are supported.", true);
  for (const path of zips) {
    if (pending.some((p) => p.path === path)) continue;
    try {
      const s = await invoke("stamp_for", { path });
      pending.push({
        path, date: s.date, time: s.time, source: s.source,
        dims: new Set(st.config ? st.config.default_dimensions : ["overworld"]),
      });
    } catch (e) { toast(String(e), true); }
  }
  render();
}

function pendingView() {
  if (!pending.length) return document.createComment("");
  return h("div", { class: "card" }, h("h2", {}, "Confirm import"),
    h("div", { class: "pending" }, pending.map(pendingCard)));
}

function pendingCard(p) {
  const hint = {
    filename: "Date and time read from the file name.",
    filename_date_only: "Date from the file name, time from the file's modified time.",
    file_time: "No date in the file name: using the file's modified time.",
  }[p.source] || "";
  const date = h("input", { type: "text", value: p.date, placeholder: "YYYY-MM-DD", "aria-label": "Date", oninput: (e) => { p.date = e.target.value.trim(); } });
  const time = h("input", { type: "text", value: p.time, placeholder: "HHMM", maxlength: 4, "aria-label": "Time (HHMM)", oninput: (e) => { p.time = e.target.value.trim(); } });
  return h("div", { class: "card", style: "background:var(--panel2)" },
    h("div", { class: "row name" }, p.path.split(/[\\/]/).pop()),
    h("div", { class: "row" }, h("label", {}, "Date ", date), h("label", {}, "Time ", time), h("small", {}, hint)),
    h("div", { class: "row" }, dimChecks(p.dims),
      h("span", { class: "actions" },
        h("button", { onclick: () => { pending = pending.filter((x) => x !== p); render(); } }, "Cancel"),
        h("button", { class: "primary", onclick: () => confirmImport(p) }, "Import"))));
}

function dimChecks(set, onchange) {
  return h("span", {}, DIMS.map(([id, name]) =>
    h("label", { class: "chk" },
      h("input", { type: "checkbox", checked: set.has(id), onchange: (e) => { e.target.checked ? set.add(id) : set.delete(id); onchange && onchange(); } }),
      name)));
}

async function confirmImport(p) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(p.date) || !/^\d{4}$/.test(p.time)) { toast("Date must be YYYY-MM-DD and time HHMM.", true); return; }
  if (!p.dims.size) { toast("Select at least one dimension.", true); return; }
  await call("enqueue_import", { args: { path: p.path, date: p.date, time: p.time, dimensions: [...p.dims] } });
  pending = pending.filter((x) => x !== p);
  render();
}

/* ---------- queue ---------- */
function queueView() {
  const jobs = [...st.jobs].reverse();
  return h("div", { class: "card" }, h("h2", {}, "Queue"),
    jobs.length ? jobs.map(jobRow) : h("div", { class: "empty" }, "Nothing queued."));
}

function jobRow(j) {
  const idx = STEPS.findIndex((s) => s[0] === j.step);
  const seeded = ["copy", "hash", "seed", "stitch"].includes(j.step);
  const stepEls = STEPS.map(([id, name], i) => {
    const on = id === j.step || (id === "render" && seeded && j.status === "running" && j.progress >= 0.3);
    const past = j.status === "done" || (idx >= 0 && i < idx);
    return h("span", { class: on ? "on" : past ? "past" : "" }, name);
  });
  return h("div", { class: "job" },
    h("div", { class: "line" }, h("span", { class: "label" }, j.label), h("span", { class: "badge " + j.status }, j.status)),
    j.status === "running" || j.status === "queued"
      ? [h("div", { class: "bar" }, h("i", { style: `width:${Math.round(j.progress * 100)}%` })),
         h("div", { class: "steps" }, stepEls), h("div", { class: "muted" }, j.detail)]
      : null,
    j.message ? h("div", { class: j.status === "error" ? "err" : "muted" }, j.message) : null);
}

/* ---------- unfinished imports (renderer crashed, disk full, ...): resume or discard ---------- */
function incompleteView() {
  const list = st.incomplete || [];
  if (!list.length) return document.createComment("");
  return h("div", { class: "card" }, h("h2", {}, "Unfinished imports"),
    h("div", { class: "muted", style: "margin-bottom:6px" }, "The tiles drawn so far were kept. Resume only draws what is still missing."),
    list.map((i) => {
      const id = i.date + "/" + i.time;
      return h("div", { class: "snap" },
        h("span", { class: "when", style: "width:auto" }, i.date + " " + fmtTime(i.time)),
        h("span", { class: "dims" }, i.source_name, i.last_error ? h("div", { class: "err" }, i.last_error.slice(0, 400)) : null),
        h("span", { class: "btns" },
          h("button", { class: "small primary", onclick: () => call("enqueue_resume", { date: i.date, time: i.time }) }, "Resume"),
          h("button", { class: "small", onclick: () => call("open_target", { kind: "snapshot_folder", date: i.date, time: i.time }) }, "Folder"),
          h("button", { class: "small danger", onclick: () => deleteSnap(i, id) }, deleteArmed === id ? "Really discard?" : "Discard")));
    }));
}

/* ---------- snapshots ---------- */
function snapshotsView() {
  const byDay = new Map();
  for (const s of st.snapshots) { if (!byDay.has(s.date)) byDay.set(s.date, []); byDay.get(s.date).push(s); }
  const days = [...byDay.keys()].sort().reverse();
  return h("div", { class: "card" }, h("h2", {}, "Snapshots"),
    days.length ? days.map((d) => h("div", { class: "day" }, h("h3", {}, d), byDay.get(d).slice().reverse().map(snapRow)))
                : h("div", { class: "empty" }, "No snapshots yet. Drop a world zip above."));
}

function snapRow(s) {
  const id = s.date + "/" + s.time;
  const missing = DIMS.map((d) => d[0]).filter((d) => !s.dimensions.includes(d));
  return h("div", { class: "snap" },
    h("span", { class: "when" }, fmtTime(s.time)),
    h("span", { class: "dims" }, s.dimensions.map(dimLabel).join(", "), h("span", { class: "muted" }, `  UTC${s.utc_offset}`)),
    h("span", { class: "btns" },
      h("button", { class: "small primary", onclick: () => call("open_target", { kind: "snapshot", date: s.date, time: s.time }) }, "Open"),
      h("button", { class: "small", onclick: () => call("open_target", { kind: "snapshot_folder", date: s.date, time: s.time }) }, "Folder"),
      h("button", { class: "small", onclick: () => { rerenderOpen = rerenderOpen === id ? null : id; rerenderDims = new Set(missing.length ? missing.slice(0, 1) : []); render(); } }, "Re-render"),
      h("button", { class: "small danger", onclick: () => deleteSnap(s, id) }, deleteArmed === id ? "Really delete?" : "Delete")),
    rerenderOpen === id
      ? h("div", { class: "rerender" }, "Render dimensions:", dimChecks(rerenderDims),
          h("button", { class: "small primary", onclick: () => doRerender(s) }, "Start"),
          h("button", { class: "small", onclick: () => { rerenderOpen = null; render(); } }, "Cancel"))
      : null);
}

async function doRerender(s) {
  if (!rerenderDims.size) { toast("Select at least one dimension.", true); return; }
  await call("enqueue_rerender", { date: s.date, time: s.time, dimensions: [...rerenderDims] });
  rerenderOpen = null; render();
}

async function deleteSnap(s, id) {
  if (deleteArmed !== id) {
    deleteArmed = id; render();
    setTimeout(() => { if (deleteArmed === id) { deleteArmed = null; render(); } }, 4000);
    return;
  }
  deleteArmed = null;
  st = await call("delete_snapshot", { date: s.date, time: s.time });
  render();
}

/* ---------- settings ---------- */
function openSettings() {
  const dlg = document.getElementById("settings");
  const cfg = JSON.parse(JSON.stringify(st.config));
  const dims = new Set(cfg.default_dimensions);
  const keep = h("input", { type: "checkbox", checked: cfg.keep_world });
  const stitched = h("input", { type: "checkbox", checked: cfg.stitched_image });
  const maxpx = h("input", { type: "text", value: String(cfg.stitched_max_px ?? 16384), style: "width:90px" });
  const threads = h("input", { type: "text", value: String(cfg.render_threads ?? 0), style: "width:70px" });

  dlg.replaceChildren(
    h("h1", {}, "Settings"),
    h("div", { class: "field" }, h("span", {}, "Default dimensions"), dimChecks(dims)),
    h("div", { class: "field" }, h("label", { class: "chk" }, keep, "Keep the extracted world after rendering"),
      h("small", { class: "muted" }, "Uses a lot of disk space. Re-render extracts it again from the zip when it is gone.")),
    h("div", { class: "field" }, h("label", { class: "chk" }, stitched, "Also write one stitched image per dimension"),
      h("div", { class: "inline" }, h("span", { class: "muted" }, "Longest side, pixels"), maxpx),
      h("small", { class: "muted" }, "The image is shrunk to fit. 16384 pixels is a good limit; larger images need a lot of memory.")),
    h("div", { class: "field" }, h("span", {}, "Render threads"), threads,
      h("small", { class: "muted" }, "0 = all processor cores but one, so the computer stays usable while a map renders.")),
    h("div", { class: "field" }, h("span", {}, "Data folder"),
      h("div", { class: "muted", style: "word-break:break-all" }, st.data_dir),
      h("button", { class: "small", style: "justify-self:start", onclick: async () => { await chooseDataFolder(); dlg.close(); } }, "Change…")),
    h("div", { class: "foot" },
      h("button", { onclick: () => dlg.close() }, "Cancel"),
      h("button", { class: "primary", onclick: async () => {
        const t = parseInt(threads.value, 10);
        if (!Number.isInteger(t) || t < 0 || t > 256) { toast("Render threads must be a number from 0 to 256.", true); return; }
        const px = parseInt(maxpx.value, 10);
        if (!Number.isInteger(px) || px < 1024 || px > 65536) { toast("The stitched image size must be between 1024 and 65536 pixels.", true); return; }
        if (!dims.size) { toast("Select at least one default dimension.", true); return; }
        cfg.default_dimensions = [...dims];
        cfg.keep_world = keep.checked; cfg.stitched_image = stitched.checked;
        cfg.stitched_max_px = px; cfg.render_threads = t;
        await call("save_config", { config: cfg });
        st.config = cfg; dlg.close(); toast("Settings saved.");
      } }, "Save")));
  dlg.showModal();
}

/* ---------- log panel (built once, lives outside #root so re-renders never reset it) ---------- */
const MAX_LOG_LINES = 1500;
let logBox = null, autoScroll = null;

function appendLog(line) {
  if (!logBox) return;
  const nearBottom = logBox.scrollHeight - logBox.scrollTop - logBox.clientHeight < 40;
  const row = h("div", { class: "ll " + line.level }, h("span", { class: "ts" }, line.ts), line.text);
  logBox.append(row);
  while (logBox.childElementCount > MAX_LOG_LINES) logBox.firstElementChild.remove();
  if (autoScroll.checked && nearBottom) logBox.scrollTop = logBox.scrollHeight;
}

function buildLogPanel() {
  logBox = h("div", { id: "logbox", tabindex: "0" });
  autoScroll = h("input", { type: "checkbox", checked: true });
  const bar = h("div", { class: "logbar" },
    h("label", { class: "chk" }, autoScroll, "Follow"),
    h("button", { class: "small", onclick: async () => {
      try { await navigator.clipboard.writeText([...logBox.children].map((r) => r.textContent).join("\n")); toast("Log copied."); }
      catch (e) { toast("Could not copy. Select the text and press Ctrl+C.", true); }
    } }, "Copy"),
    h("button", { class: "small", onclick: () => { logBox.replaceChildren(); } }, "Clear view"),
    h("button", { class: "small", onclick: () => call("open_target", { kind: "log" }) }, "Open log file"),
    h("span", { class: "spacer" }),
    h("small", { class: "muted" }, "Also saved to logs/app.log in the data folder"));
  let wrap = document.getElementById("logwrap");
  if (!wrap) {   // e.g. a stale index.html: create the container instead of crashing
    wrap = h("div", { id: "logwrap" });
    document.body.append(wrap);
  }
  wrap.append(h("details", { open: true }, h("summary", {}, "Log"), bar, logBox));
  invoke("get_log").then((lines) => { lines.forEach(appendLog); logBox.scrollTop = logBox.scrollHeight; }).catch(() => {});
}

/* ---------- events ---------- */
async function refresh() { st = await call("get_state"); render(); }

// Renderer output can arrive many times per second: draw at most once per frame.
let renderQueued = false;
function scheduleRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => { renderQueued = false; render(); });
}

listen("queue-update", (e) => { st.jobs = e.payload; scheduleRender(); });
listen("log-line", (e) => appendLog(e.payload));
listen("state-changed", refresh);
listen("tauri://drag-enter", () => { const d = document.getElementById("drop"); if (d) d.classList.add("over"); });
listen("tauri://drag-leave", () => { const d = document.getElementById("drop"); if (d) d.classList.remove("over"); });
listen("tauri://drag-drop", (e) => {
  const d = document.getElementById("drop"); if (d) d.classList.remove("over");
  if (st.data_dir && e.payload && e.payload.paths) addDrops(e.payload.paths);
});

function showFatal(msg) {
  const root = document.getElementById("root");
  root.replaceChildren(h("div", { class: "card" }, h("h2", {}, "Something went wrong"), h("div", { class: "err" }, String(msg))));
}
window.addEventListener("error", (e) => toast("UI error: " + e.message, true));
window.addEventListener("unhandledrejection", (e) => toast("UI error: " + (e.reason && e.reason.message ? e.reason.message : e.reason), true));

try { buildLogPanel(); } catch (e) { console.error("log panel failed", e); }
try { render(); } catch (e) { showFatal("Could not draw the UI: " + e); }   // before the backend answers
refresh().catch((e) => showFatal("Could not load the app state: " + e));
