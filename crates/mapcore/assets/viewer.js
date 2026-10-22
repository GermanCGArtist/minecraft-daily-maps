/* Minecraft Daily Maps viewer.
 *
 * One script, two modes (set by window.MAPVIEW.mode):
 *   "slider"   - all days, day slider, uses each day's latest snapshot
 *   "snapshot" - one snapshot page with a dropdown for the day's snapshots
 *
 * Data comes from a <script> that sets window.MAPVIEW so the pages also work
 * when opened straight from disk (file:// blocks fetch() of local JSON).
 *
 * Coordinate convention (see README): tile (x, y) at zoom z covers the block
 * rectangle [x*bpt, (x+1)*bpt) x [y*bpt, (y+1)*bpt) with bpt = tileSize / 2^z.
 * X grows east, Z (tile y) grows south.
 */
(function (root) {
  'use strict';

  var DIM_ORDER = ['overworld', 'nether', 'end'];
  var DIM_LABEL = { overworld: 'Overworld', nether: 'Nether', end: 'The End' };
  var KIND_COLOR = { a: '#2ecc71', m: '#f39c12', r: '#e74c3c' };
  var KIND_LABEL = { a: 'added', m: 'modified', r: 'removed' };

  /* ---------- pure helpers (unit-tested in Node) ---------- */

  function blocksPerTile(z, tileSize) {
    return tileSize / Math.pow(2, z);
  }

  function clamp(v, lo, hi) {
    return Math.max(lo, Math.min(hi, v));
  }

  function chooseZoom(scale, zmin, zmax) {
    var z = Math.round(Math.log(scale) / Math.LN2);
    return clamp(z, zmin, zmax);
  }

  /** Screen point -> world block coordinates. */
  function screenToWorld(view, sx, sy, w, h) {
    return {
      x: view.cx + (sx - w / 2) / view.scale,
      z: view.cz + (sy - h / 2) / view.scale
    };
  }

  /** World block coordinates -> screen point. */
  function worldToScreen(view, x, z, w, h) {
    return {
      x: (x - view.cx) * view.scale + w / 2,
      y: (z - view.cz) * view.scale + h / 2
    };
  }

  /** Range of tile indices overlapping the viewport, limited by bounds. */
  function visibleRange(view, z, tileSize, w, h, bounds) {
    var bpt = blocksPerTile(z, tileSize);
    var tl = screenToWorld(view, 0, 0, w, h);
    var br = screenToWorld(view, w, h, w, h);
    var r = {
      x0: Math.floor(tl.x / bpt),
      x1: Math.floor(br.x / bpt),
      y0: Math.floor(tl.z / bpt),
      y1: Math.floor(br.z / bpt)
    };
    if (bounds) {
      r.x0 = Math.max(r.x0, bounds.min_x);
      r.x1 = Math.min(r.x1, bounds.max_x);
      r.y0 = Math.max(r.y0, bounds.min_y);
      r.y1 = Math.min(r.y1, bounds.max_y);
    }
    return r;
  }

  /** A view that fits the given tile bounds into a w x h viewport. */
  function fitView(bounds, z, tileSize, w, h, zmin, zmax) {
    var bpt = blocksPerTile(z, tileSize);
    var bw = (bounds.max_x - bounds.min_x + 1) * bpt;
    var bh = (bounds.max_y - bounds.min_y + 1) * bpt;
    var scale = Math.min(w / bw, h / bh) * 0.92;
    return {
      cx: (bounds.min_x * bpt + (bounds.max_x + 1) * bpt) / 2,
      cz: (bounds.min_y * bpt + (bounds.max_y + 1) * bpt) / 2,
      scale: clamp(scale, scaleLimits(zmin, zmax).min, scaleLimits(zmin, zmax).max)
    };
  }

  function scaleLimits(zmin, zmax) {
    return { min: Math.pow(2, zmin - 2), max: Math.pow(2, zmax + 3) };
  }

  /** Zoom the view by `factor`, keeping the world point under (sx, sy) fixed. */
  function zoomAt(view, factor, sx, sy, w, h, zmin, zmax) {
    var lim = scaleLimits(zmin, zmax);
    var ns = clamp(view.scale * factor, lim.min, lim.max);
    var before = screenToWorld(view, sx, sy, w, h);
    var next = { cx: view.cx, cz: view.cz, scale: ns };
    var after = screenToWorld(next, sx, sy, w, h);
    next.cx += before.x - after.x;
    next.cz += before.z - after.z;
    return next;
  }

  function formatLabel(entry) {
    var t = entry.time || '';
    var hhmm = t.length === 4 ? t.slice(0, 2) + ':' + t.slice(2) : t;
    return entry.date + ' ' + hhmm;
  }

  function dimsOf(entry) {
    var keys = Object.keys(entry.dimensions || {});
    keys.sort(function (a, b) {
      return DIM_ORDER.indexOf(a) - DIM_ORDER.indexOf(b);
    });
    return keys;
  }

  function tileUrl(prefix, entry, dim, z, x, y) {
    return prefix + entry.base + '/render/tiles/' + dim + '/' + z + '/' + x + '/' + y + '.png';
  }

  var pure = {
    blocksPerTile: blocksPerTile,
    chooseZoom: chooseZoom,
    screenToWorld: screenToWorld,
    worldToScreen: worldToScreen,
    visibleRange: visibleRange,
    fitView: fitView,
    zoomAt: zoomAt,
    scaleLimits: scaleLimits,
    formatLabel: formatLabel,
    dimsOf: dimsOf,
    tileUrl: tileUrl
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = pure;
  }

  /* ---------- viewer ---------- */

  function el(tag, attrs, children) {
    var e = root.document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === 'text') e.textContent = attrs[k];
      else if (k === 'class') e.className = attrs[k];
      else e.setAttribute(k, attrs[k]);
    });
    (children || []).forEach(function (c) { e.appendChild(c); });
    return e;
  }

  function init(data) {
    var doc = root.document;
    var app = doc.getElementById('app');
    if (!app) return;
    var entries = data.entries || [];
    var prefix = data.prefix || '';
    var slider = data.mode === 'slider';

    var state = {
      entryIdx: entries.length - 1,
      dim: null,
      showChanges: false,
      views: {},
      view: { cx: 0, cz: 0, scale: 1 },
      mouse: null
    };

    /* DOM */
    var title = el('div', { class: 'title', text: data.title || 'Minecraft map' });
    var bar = el('header', { class: 'bar' }, [title]);
    var dayLabel = el('span', { class: 'daylabel' });
    var range = null, prevBtn = null, nextBtn = null, snapSelect = null, dayLink = null;

    if (slider) {
      prevBtn = el('button', { class: 'btn', title: 'Previous day', text: '◀' });
      nextBtn = el('button', { class: 'btn', title: 'Next day', text: '▶' });
      range = el('input', { type: 'range', min: '0', max: String(Math.max(0, entries.length - 1)), step: '1', value: String(state.entryIdx), class: 'range', 'aria-label': 'Day' });
      dayLink = el('a', { class: 'link', text: 'Day page', href: '#' });
      bar.appendChild(prevBtn);
      bar.appendChild(range);
      bar.appendChild(nextBtn);
      bar.appendChild(dayLabel);
      bar.appendChild(dayLink);
    } else {
      bar.appendChild(dayLabel);
      var sibs = data.siblings || [];
      if (sibs.length > 1) {
        snapSelect = el('select', { class: 'select', 'aria-label': 'Snapshot' });
        sibs.forEach(function (s) {
          var o = el('option', { value: s.href, text: s.label });
          if (s.current) o.selected = true;
          snapSelect.appendChild(o);
        });
        bar.appendChild(snapSelect);
      }
    }

    var tabs = el('div', { class: 'tabs' });
    bar.appendChild(tabs);
    var changesBox = el('input', { type: 'checkbox', id: 'chg' });
    var changesLabel = el('label', { class: 'chk', for: 'chg' }, [changesBox, el('span', { text: 'Show changes' })]);
    var legend = el('span', { class: 'legend' });
    bar.appendChild(changesLabel);
    bar.appendChild(legend);
    var zoomOut = el('button', { class: 'btn', title: 'Zoom out', text: '−' });
    var zoomIn = el('button', { class: 'btn', title: 'Zoom in', text: '+' });
    var fitBtn = el('button', { class: 'btn', title: 'Fit map', text: 'Fit' });
    bar.appendChild(zoomOut);
    bar.appendChild(zoomIn);
    bar.appendChild(fitBtn);

    var stage = el('main', { class: 'stage' });
    var canvas = el('canvas', { class: 'map' });
    var coords = el('div', { class: 'coords', text: '' });
    var empty = el('div', { class: 'empty', text: '' });
    stage.appendChild(canvas);
    stage.appendChild(coords);
    stage.appendChild(empty);
    app.appendChild(bar);
    app.appendChild(stage);

    var ctx = canvas.getContext('2d');
    var cssW = 1, cssH = 1, dpr = 1;
    var dirty = true;

    function entry() { return entries[state.entryIdx]; }
    function dimMeta() { var e = entry(); return e && state.dim ? e.dimensions[state.dim] : null; }
    function invalidate() { dirty = true; }

    /* tile cache (insertion-ordered Map used as a simple LRU) */
    var cache = new Map();
    var CACHE_MAX = 900;
    function getTile(url) {
      var hit = cache.get(url);
      if (hit) {
        cache.delete(url);
        cache.set(url, hit);
        return hit;
      }
      var rec = { img: new root.Image(), state: 'loading' };
      rec.img.onload = function () { rec.state = 'ok'; invalidate(); };
      rec.img.onerror = function () { rec.state = 'err'; };
      rec.img.src = url;
      cache.set(url, rec);
      if (cache.size > CACHE_MAX) {
        var oldest = cache.keys().next().value;
        cache.delete(oldest);
      }
      return rec;
    }

    /* change maps per (entry, dim, zoom) */
    var changeCache = {};
    function changeMap(z) {
      var e = entry();
      var key = state.entryIdx + '|' + state.dim + '|' + z;
      if (changeCache[key]) return changeCache[key];
      var m = new Map();
      var byDim = (e.changes || {})[state.dim] || {};
      (byDim[String(z)] || []).forEach(function (t) { m.set(t[0] + '/' + t[1], t[2]); });
      changeCache[key] = m;
      return m;
    }

    function resize() {
      var r = stage.getBoundingClientRect();
      dpr = root.devicePixelRatio || 1;
      cssW = Math.max(1, Math.floor(r.width));
      cssH = Math.max(1, Math.floor(r.height));
      canvas.width = Math.floor(cssW * dpr);
      canvas.height = Math.floor(cssH * dpr);
      canvas.style.width = cssW + 'px';
      canvas.style.height = cssH + 'px';
      invalidate();
    }

    function currentZoom() {
      var m = dimMeta();
      if (!m) return 0;
      return chooseZoom(state.view.scale, m.zoom_min, m.zoom_max);
    }

    function drawTiles() {
      var e = entry(), m = dimMeta();
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, cssW, cssH);
      if (!e || !m) return;
      var z = currentZoom();
      var bounds = m.bounds[String(z)];
      if (!bounds) return;
      var ts = m.tile_size;
      var bpt = blocksPerTile(z, ts);
      var r = visibleRange(state.view, z, ts, cssW, cssH, bounds);
      var size = bpt * state.view.scale;
      var cm = state.showChanges ? changeMap(z) : null;
      ctx.imageSmoothingEnabled = state.view.scale < Math.pow(2, z) ? true : false;
      var counts = { a: 0, m: 0, r: 0 };
      for (var ty = r.y0; ty <= r.y1; ty++) {
        for (var tx = r.x0; tx <= r.x1; tx++) {
          var p = worldToScreen(state.view, tx * bpt, ty * bpt, cssW, cssH);
          var px = Math.floor(p.x), py = Math.floor(p.y);
          var pw = Math.ceil(p.x + size) - px, ph = Math.ceil(p.y + size) - py;
          var kind = cm ? cm.get(tx + '/' + ty) : null;
          var rec = getTile(tileUrl(prefix, e, state.dim, z, tx, ty));
          if (rec.state === 'ok') {
            ctx.drawImage(rec.img, px, py, pw, ph);
          }
          if (kind) {
            counts[kind]++;
            ctx.save();
            ctx.globalAlpha = 0.14;
            ctx.fillStyle = KIND_COLOR[kind];
            if (rec.state === 'ok') ctx.fillRect(px, py, pw, ph);
            ctx.restore();
            ctx.lineWidth = 2;
            ctx.strokeStyle = KIND_COLOR[kind];
            if (kind === 'r' && ctx.setLineDash) ctx.setLineDash([6, 4]);
            ctx.strokeRect(px + 1, py + 1, Math.max(1, pw - 2), Math.max(1, ph - 2));
            if (ctx.setLineDash) ctx.setLineDash([]);
          }
        }
      }
      if (state.showChanges) {
        var total = 0;
        var cmAll = changeMap(z);
        var all = { a: 0, m: 0, r: 0 };
        cmAll.forEach(function (k) { all[k]++; total++; });
        legend.textContent = total === 0 ? 'no changes' :
          '+' + all.a + ' ~' + all.m + ' −' + all.r + ' tiles (zoom ' + z + ')';
      } else {
        legend.textContent = '';
      }
    }

    function loop() {
      if (dirty) {
        dirty = false;
        try { drawTiles(); } catch (err) { if (root.console) root.console.error(err); }
      }
      root.requestAnimationFrame(loop);
    }

    /* selection logic */
    function chooseDim(preferred) {
      var dims = dimsOf(entry());
      if (preferred && dims.indexOf(preferred) >= 0) return preferred;
      return dims[0] || null;
    }

    function saveView() {
      if (state.dim) state.views[state.dim] = { cx: state.view.cx, cz: state.view.cz, scale: state.view.scale };
    }

    function fitCurrent() {
      var m = dimMeta();
      if (!m) return;
      var zf = m.zoom_min;
      var b = m.bounds[String(zf)];
      if (!b) return;
      state.view = fitView(b, zf, m.tile_size, cssW, cssH, m.zoom_min, m.zoom_max);
    }

    function renderTabs() {
      tabs.textContent = '';
      dimsOf(entry()).forEach(function (d) {
        var b = el('button', { class: 'tab' + (d === state.dim ? ' on' : ''), text: DIM_LABEL[d] || d });
        b.addEventListener('click', function () { setDim(d); });
        tabs.appendChild(b);
      });
    }

    function setDim(d) {
      if (d === state.dim) return;
      saveView();
      state.dim = d;
      if (state.views[d]) state.view = state.views[d];
      else fitCurrent();
      renderTabs();
      refreshEmpty();
      invalidate();
    }

    function refreshEmpty() {
      var m = dimMeta();
      empty.textContent = m ? '' : 'No rendered dimensions in this snapshot.';
    }

    function setEntry(idx) {
      idx = clamp(idx, 0, entries.length - 1);
      saveView();
      var prevDim = state.dim;
      state.entryIdx = idx;
      state.dim = chooseDim(prevDim);
      if (state.views[state.dim]) state.view = state.views[state.dim];
      else fitCurrent();
      var e = entry();
      dayLabel.textContent = formatLabel(e) + (e.utc_offset ? ' (UTC' + e.utc_offset + ')' : '') +
        (slider && e.snapshots && e.snapshots.length > 1 ? ' · ' + e.snapshots.length + ' snapshots' : '');
      if (range) range.value = String(idx);
      if (dayLink) dayLink.setAttribute('href', prefix + e.base + '/index.html');
      if (prevBtn) prevBtn.disabled = idx <= 0;
      if (nextBtn) nextBtn.disabled = idx >= entries.length - 1;
      renderTabs();
      refreshEmpty();
      invalidate();
    }

    function zoomBy(factor, sx, sy) {
      var m = dimMeta();
      if (!m) return;
      state.view = zoomAt(state.view, factor, sx == null ? cssW / 2 : sx, sy == null ? cssH / 2 : sy, cssW, cssH, m.zoom_min, m.zoom_max);
      invalidate();
    }

    /* events */
    if (range) {
      range.addEventListener('input', function () { setEntry(parseInt(range.value, 10)); });
      prevBtn.addEventListener('click', function () { setEntry(state.entryIdx - 1); });
      nextBtn.addEventListener('click', function () { setEntry(state.entryIdx + 1); });
    }
    if (snapSelect) {
      snapSelect.addEventListener('change', function () { root.location.href = snapSelect.value; });
    }
    changesBox.addEventListener('change', function () { state.showChanges = changesBox.checked; invalidate(); });
    zoomIn.addEventListener('click', function () { zoomBy(2); });
    zoomOut.addEventListener('click', function () { zoomBy(0.5); });
    fitBtn.addEventListener('click', function () { fitCurrent(); invalidate(); });

    var pointers = new Map();
    var lastPinch = 0;
    canvas.addEventListener('pointerdown', function (ev) {
      pointers.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
      if (canvas.setPointerCapture) { try { canvas.setPointerCapture(ev.pointerId); } catch (e) { /* ignore */ } }
      lastPinch = 0;
      canvas.classList.add('drag');
    });
    canvas.addEventListener('pointermove', function (ev) {
      var rect = canvas.getBoundingClientRect();
      var sx = ev.clientX - rect.left, sy = ev.clientY - rect.top;
      state.mouse = { sx: sx, sy: sy };
      var w = screenToWorld(state.view, sx, sy, cssW, cssH);
      coords.textContent = 'X ' + Math.floor(w.x) + '   Z ' + Math.floor(w.z);
      var prev = pointers.get(ev.pointerId);
      if (!prev) return;
      if (pointers.size === 1) {
        state.view.cx -= (ev.clientX - prev.x) / state.view.scale;
        state.view.cz -= (ev.clientY - prev.y) / state.view.scale;
        invalidate();
      } else if (pointers.size === 2) {
        pointers.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
        var pts = Array.from(pointers.values());
        var dist = Math.hypot(pts[0].x - pts[1].x, pts[0].y - pts[1].y);
        if (lastPinch > 0 && dist > 0) {
          zoomBy(dist / lastPinch, (pts[0].x + pts[1].x) / 2 - rect.left, (pts[0].y + pts[1].y) / 2 - rect.top);
        }
        lastPinch = dist;
      }
      pointers.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
    });
    function endPointer(ev) {
      pointers.delete(ev.pointerId);
      lastPinch = 0;
      if (pointers.size === 0) canvas.classList.remove('drag');
    }
    canvas.addEventListener('pointerup', endPointer);
    canvas.addEventListener('pointercancel', endPointer);
    canvas.addEventListener('pointerleave', function () { coords.textContent = ''; });
    canvas.addEventListener('wheel', function (ev) {
      ev.preventDefault();
      var rect = canvas.getBoundingClientRect();
      zoomBy(Math.exp(-ev.deltaY * 0.0015), ev.clientX - rect.left, ev.clientY - rect.top);
    }, { passive: false });
    doc.addEventListener('keydown', function (ev) {
      var t = ev.target && ev.target.tagName;
      if (t === 'INPUT' || t === 'SELECT' || t === 'TEXTAREA') return;
      if (ev.key === 'ArrowLeft' && slider) setEntry(state.entryIdx - 1);
      else if (ev.key === 'ArrowRight' && slider) setEntry(state.entryIdx + 1);
      else if (ev.key === '+' || ev.key === '=') zoomBy(2);
      else if (ev.key === '-') zoomBy(0.5);
      else if (ev.key.toLowerCase() === 'c') { changesBox.checked = !changesBox.checked; state.showChanges = changesBox.checked; invalidate(); }
    });
    if (root.ResizeObserver) new root.ResizeObserver(resize).observe(stage);
    root.addEventListener('resize', resize);

    if (!entries.length) {
      empty.textContent = 'No snapshots yet.';
      return;
    }
    resize();
    setEntry(state.entryIdx);
    root.requestAnimationFrame(loop);
    root.__mapviewer = { state: state, setEntry: setEntry, setDim: setDim, zoomBy: zoomBy };
  }

  if (root.window === root && root.MAPVIEW) {
    var start = function () { init(root.MAPVIEW); };
    if (root.document.readyState === 'loading') root.document.addEventListener('DOMContentLoaded', start);
    else start();
  }
  pure.init = init;
})(typeof window !== 'undefined' ? window : globalThis);
