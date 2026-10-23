/* Minecraft Daily Maps viewer. Works from file:// (data comes in as window.* scripts).
 *
 * Tile geometry (the app's own renderer):
 *   - tile_size is 512 pixels; at zoom level 0 one pixel is one block, so a tile is one region
 *   - a tile at zoom level z covers  tile_size / 2^z  blocks per side (z = 0, -1, -2, ...)
 *   - tile (x, y) has its north-west corner at block (x * span, y * span); y grows towards +Z
 * Older snapshots (uNmINeD, tile_size 256) use the same formulas with their own tile_size.
 */
(function () {
  "use strict";
  var V = window.MC_VIEW;
  var T = V.tile_size || 256;
  var $ = function (id) { return document.getElementById(id); };
  var mapEl = $("map"), tilesEl = $("tiles"), overlayEl = $("overlay"), emptyEl = $("empty");

  var state = {
    id: null,          // "2026-10-22/0005"
    dim: null,
    scale: 1,          // screen pixels per block
    cx: 0, cz: 0,      // block at the centre of the screen
    data: null,        // parsed tiles of the current snapshot
    diff: {},          // dimension -> {z, added, changed, removed}
    show: false,
    fitted: false
  };
  var pool = new Map();
  var loaded = {};     // script urls already loaded

  function loadScript(src) {
    if (loaded[src]) return loaded[src];
    loaded[src] = new Promise(function (res, rej) {
      var s = document.createElement("script");
      s.src = src; s.onload = res;
      s.onerror = function () { delete loaded[src]; rej(new Error("Could not load " + src)); };
      document.head.appendChild(s);
    });
    return loaded[src];
  }

  var site = V.mode === "site";
  function tilesBase(id) { return (site ? "../days/" + id + "/" : "") + "render/tiles/"; }
  function tilesScript(id) { return (site ? "../days/" + id + "/" : "") + "render/tiles-data.js"; }

  /* ---------- data ---------- */
  function parseTiles(raw) {
    var out = {};
    Object.keys(raw.dimensions).forEach(function (dim) {
      var levels = new Map(), zmin = Infinity, zmax = -Infinity;
      raw.dimensions[dim].levels.forEach(function (lv) {
        var m = new Map();
        lv.tiles.forEach(function (t) { m.set(t[0] + "," + t[1], t[2]); });
        levels.set(lv.z, m);
        zmin = Math.min(zmin, lv.z); zmax = Math.max(zmax, lv.z);
      });
      out[dim] = { levels: levels, zmin: zmin, zmax: zmax };
    });
    return out;
  }

  function span(z) { return T / Math.pow(2, z); }

  function bounds(dd) {
    var m = dd.levels.get(dd.zmin), s = span(dd.zmin);
    var x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    m.forEach(function (_, k) {
      var p = k.split(","), x = +p[0], y = +p[1];
      x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x + 1); y1 = Math.max(y1, y + 1);
    });
    return { x0: x0 * s, z0: y0 * s, x1: x1 * s, z1: y1 * s };
  }

  function fit() {
    var dd = state.data && state.data[state.dim];
    if (!dd) return;
    var b = bounds(dd), w = mapEl.clientWidth || 800, h = mapEl.clientHeight || 600;
    var bw = Math.max(1, b.x1 - b.x0), bh = Math.max(1, b.z1 - b.z0);
    state.scale = clamp(Math.min(w / bw, h / bh) * 0.92, 1 / 128, 64);
    state.cx = (b.x0 + b.x1) / 2; state.cz = (b.z0 + b.z1) / 2;
    state.fitted = true;
    render();
  }

  function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }

  /* ---------- rendering ---------- */
  function clearTiles() {
    pool.forEach(function (img) { img.remove(); });
    pool.clear();
    overlayEl.innerHTML = "";
  }

  function render() {
    var dd = state.data && state.data[state.dim];
    if (!dd) { clearTiles(); return; }
    var w = mapEl.clientWidth, h = mapEl.clientHeight, sc = state.scale;
    var z = clamp(Math.round(Math.log2(sc)), dd.zmin, dd.zmax);
    var lvl = dd.levels.get(z) || dd.levels.get(dd.zmin);
    var sp = span(z), px = sp * sc;
    var bx0 = state.cx - w / 2 / sc, bz0 = state.cz - h / 2 / sc;
    var bx1 = state.cx + w / 2 / sc, bz1 = state.cz + h / 2 / sc;
    var tx0 = Math.floor(bx0 / sp), tx1 = Math.floor(bx1 / sp);
    var ty0 = Math.floor(bz0 / sp), ty1 = Math.floor(bz1 / sp);
    var needed = new Set();

    if ((tx1 - tx0 + 1) * (ty1 - ty0 + 1) <= 6000) {
      for (var ty = ty0; ty <= ty1; ty++) {
        for (var tx = tx0; tx <= tx1; tx++) {
          var path = lvl.get(tx + "," + ty);
          if (!path) continue;
          var key = z + "/" + tx + "/" + ty;
          needed.add(key);
          var img = pool.get(key);
          if (!img) {
            img = new Image();
            img.className = "tile"; img.draggable = false; img.alt = "";
            img.src = tilesBase(state.id) + state.dim + "/" + encodePath(path);
            tilesEl.appendChild(img);
            pool.set(key, img);
          }
          img.style.transform = "translate(" + ((tx * sp - bx0) * sc) + "px," + ((ty * sp - bz0) * sc) + "px)";
          img.style.width = img.style.height = px + "px";
          img.style.imageRendering = sc >= 1.5 ? "pixelated" : "auto";
        }
      }
    }
    pool.forEach(function (img, key) {
      if (!needed.has(key)) { img.remove(); pool.delete(key); }
    });
    renderOverlay(bx0, bz0, bx1, bz1);
  }

  function encodePath(p) { return p.split("/").map(encodeURIComponent).join("/"); }

  function renderOverlay(bx0, bz0, bx1, bz1) {
    overlayEl.innerHTML = "";
    $("legend").hidden = !state.show;
    if (!state.show) return;
    var d = state.diff[state.dim];
    if (!d) return;
    var sp = span(d.z), sc = state.scale, frag = document.createDocumentFragment(), n = 0;
    [["a", d.added], ["c", d.changed], ["r", d.removed]].forEach(function (kind) {
      kind[1].forEach(function (t) {
        if (n > 4000) return;
        var x = t[0] * sp, y = t[1] * sp;
        if (x + sp < bx0 || x > bx1 || y + sp < bz0 || y > bz1) return;
        var b = document.createElement("div");
        b.className = "box " + kind[0];
        b.style.left = ((x - bx0) * sc) + "px"; b.style.top = ((y - bz0) * sc) + "px";
        b.style.width = b.style.height = (sp * sc) + "px";
        frag.appendChild(b); n++;
      });
    });
    overlayEl.appendChild(frag);
  }

  /* ---------- selecting snapshots / dimensions ---------- */
  function setDims(dims) {
    var tabs = $("dimTabs");
    tabs.innerHTML = "";
    dims.forEach(function (d) {
      var b = document.createElement("button");
      b.textContent = d.charAt(0).toUpperCase() + d.slice(1);
      b.setAttribute("role", "tab");
      b.setAttribute("aria-selected", d === state.dim ? "true" : "false");
      b.onclick = function () { selectDim(d); };
      tabs.appendChild(b);
    });
    tabs.hidden = dims.length < 2;
  }

  function selectDim(d) {
    if (d === state.dim) return;
    state.dim = d; clearTiles();
    Array.prototype.forEach.call($("dimTabs").children, function (b) {
      b.setAttribute("aria-selected", b.textContent.toLowerCase() === d ? "true" : "false");
    });
    fit();
  }

  function showEmpty(msg) { emptyEl.hidden = !msg; emptyEl.textContent = msg || ""; }

  function openSnapshot(id, dims, diff) {
    showEmpty("Loading…");
    return loadScript(tilesScript(id)).then(function () {
      state.id = id;
      T = window.MC_TILES[id].tile_size || V.tile_size || 256;
      state.data = parseTiles(window.MC_TILES[id]);
      state.diff = diff || {};
      var avail = dims.filter(function (d) { return state.data[d]; });
      if (!avail.length) { showEmpty("No tiles in this snapshot."); clearTiles(); return; }
      var keep = state.dim && avail.indexOf(state.dim) >= 0;
      if (!keep) state.dim = avail[0];
      setDims(avail);
      clearTiles(); showEmpty("");
      if (!state.fitted || !keep) fit(); else render();
    }).catch(function (e) { showEmpty(e.message); });
  }

  /* ---------- site mode: day slider ---------- */
  function fmtOffset(o) { return o ? " (UTC" + o + ")" : ""; }

  function initSite() {
    $("siteControls").hidden = false;
    var days = V.days;
    if (!days.length) { showEmpty("No snapshots yet. Import a world zip in the app."); $("chgLabel").hidden = true; return; }
    var slider = $("slider");
    slider.max = String(days.length - 1);
    slider.value = String(days.length - 1);
    var current = -1, snapId = null;

    function pick(i, id) {
      current = i = clamp(i, 0, days.length - 1);
      var day = days[i];
      snapId = id || day.snapshots[day.snapshots.length - 1].id;
      slider.value = String(i);
      $("dayLabel").textContent = day.date;
      var sel = $("siteSnap");
      sel.innerHTML = "";
      day.snapshots.forEach(function (s) {
        var o = document.createElement("option");
        o.value = s.id; o.textContent = s.time.slice(0, 2) + ":" + s.time.slice(2);
        if (s.id === snapId) o.selected = true;
        sel.appendChild(o);
      });
      sel.hidden = day.snapshots.length < 2;
      var snap = day.snapshots.filter(function (s) { return s.id === snapId; })[0];
      // The slider shows a day's combined changes; a specific snapshot loads its own diff file.
      var useDay = snapId === day.snapshots[day.snapshots.length - 1].id;
      if (useDay) {
        openSnapshot(snapId, snap.dimensions, day.changes);
      } else {
        loadScript("../days/" + snapId + "/diff-data.js").then(function () {
          openSnapshot(snapId, snap.dimensions, window.MC_DIFF[snapId]);
        });
      }
    }
    slider.oninput = function () { pick(+slider.value); };
    $("prevDay").onclick = function () { pick(current - 1); };
    $("nextDay").onclick = function () { pick(current + 1); };
    $("siteSnap").onchange = function () { pick(current, this.value); };
    pick(days.length - 1);
  }

  /* ---------- snapshot mode ---------- */
  function initSnapshot() {
    $("snapControls").hidden = false;
    $("snapLabel").textContent = V.date + "  " + V.time.slice(0, 2) + ":" + V.time.slice(2) + fmtOffset(V.utc_offset);
    var sel = $("snapSel");
    V.siblings.forEach(function (s) {
      var o = document.createElement("option");
      o.value = s.href; o.textContent = s.time.slice(0, 2) + ":" + s.time.slice(2);
      if (s.time === V.time) o.selected = true;
      sel.appendChild(o);
    });
    sel.hidden = V.siblings.length < 2;
    sel.onchange = function () { location.href = this.value; };
    loadScript("diff-data.js").catch(function () {}).then(function () {
      openSnapshot(V.id, V.dimensions, (window.MC_DIFF || {})[V.id]);
    });
  }

  /* ---------- interaction ---------- */
  var drag = null;
  mapEl.addEventListener("pointerdown", function (e) {
    drag = { x: e.clientX, y: e.clientY, cx: state.cx, cz: state.cz };
    mapEl.setPointerCapture(e.pointerId); mapEl.classList.add("dragging");
  });
  mapEl.addEventListener("pointermove", function (e) {
    var r = mapEl.getBoundingClientRect();
    var bx = state.cx + (e.clientX - r.left - r.width / 2) / state.scale;
    var bz = state.cz + (e.clientY - r.top - r.height / 2) / state.scale;
    $("coords").textContent = "X: " + Math.floor(bx) + "   Z: " + Math.floor(bz);
    if (!drag) return;
    state.cx = drag.cx - (e.clientX - drag.x) / state.scale;
    state.cz = drag.cz - (e.clientY - drag.y) / state.scale;
    render();
  });
  function endDrag() { drag = null; mapEl.classList.remove("dragging"); }
  mapEl.addEventListener("pointerup", endDrag);
  mapEl.addEventListener("pointercancel", endDrag);

  function zoomAt(factor, px, py) {
    var r = mapEl.getBoundingClientRect();
    var ox = px === undefined ? r.width / 2 : px, oy = py === undefined ? r.height / 2 : py;
    var bx = state.cx + (ox - r.width / 2) / state.scale, bz = state.cz + (oy - r.height / 2) / state.scale;
    state.scale = clamp(state.scale * factor, 1 / 128, 64);
    state.cx = bx - (ox - r.width / 2) / state.scale;
    state.cz = bz - (oy - r.height / 2) / state.scale;
    render();
  }
  mapEl.addEventListener("wheel", function (e) {
    e.preventDefault();
    var r = mapEl.getBoundingClientRect();
    zoomAt(Math.pow(1.0015, -e.deltaY), e.clientX - r.left, e.clientY - r.top);
  }, { passive: false });
  $("zoomIn").onclick = function () { zoomAt(1.5); };
  $("zoomOut").onclick = function () { zoomAt(1 / 1.5); };
  $("fit").onclick = fit;
  $("chgToggle").onchange = function () { state.show = this.checked; render(); };
  window.addEventListener("resize", render);

  if (site) initSite(); else initSnapshot();
})();
