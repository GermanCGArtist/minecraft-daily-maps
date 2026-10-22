'use strict';
const test = require('node:test');
const assert = require('node:assert');
const path = require('node:path');

const VIEWER = path.join(__dirname, '..', '..', 'crates', 'mapcore', 'assets', 'viewer.js');

/* ---------- pure math ---------- */
const pure = require(VIEWER);

test('blocksPerTile follows 2^-z', () => {
  assert.strictEqual(pure.blocksPerTile(0, 256), 256);
  assert.strictEqual(pure.blocksPerTile(1, 256), 128);
  assert.strictEqual(pure.blocksPerTile(-2, 256), 1024);
});

test('chooseZoom rounds and clamps', () => {
  assert.strictEqual(pure.chooseZoom(1, -3, 2), 0);
  assert.strictEqual(pure.chooseZoom(0.26, -3, 2), -2);
  assert.strictEqual(pure.chooseZoom(100, -3, 2), 2);
  assert.strictEqual(pure.chooseZoom(0.001, -3, 2), -3);
});

test('screen/world conversions are inverse', () => {
  const v = { cx: 120, cz: -40, scale: 2.5 };
  const w = pure.screenToWorld(v, 300, 200, 800, 600);
  const s = pure.worldToScreen(v, w.x, w.z, 800, 600);
  assert.ok(Math.abs(s.x - 300) < 1e-9 && Math.abs(s.y - 200) < 1e-9);
  const c = pure.screenToWorld(v, 400, 300, 800, 600);
  assert.deepStrictEqual([c.x, c.z], [120, -40]);
});

test('zoomAt keeps the anchor point fixed', () => {
  const v = { cx: 0, cz: 0, scale: 1 };
  const before = pure.screenToWorld(v, 600, 100, 800, 600);
  const n = pure.zoomAt(v, 2, 600, 100, 800, 600, -2, 2);
  const after = pure.screenToWorld(n, 600, 100, 800, 600);
  assert.ok(Math.abs(before.x - after.x) < 1e-9 && Math.abs(before.z - after.z) < 1e-9);
  assert.strictEqual(n.scale, 2);
});

test('zoomAt clamps to limits', () => {
  const lim = pure.scaleLimits(-2, 1);
  const n = pure.zoomAt({ cx: 0, cz: 0, scale: lim.max }, 10, 0, 0, 100, 100, -2, 1);
  assert.strictEqual(n.scale, lim.max);
});

test('visibleRange is limited by bounds', () => {
  const v = { cx: 0, cz: 0, scale: 1 };
  const r = pure.visibleRange(v, 0, 256, 1024, 1024, { min_x: -1, max_x: 1, min_y: 0, max_y: 0 });
  assert.deepStrictEqual(r, { x0: -1, x1: 1, y0: 0, y1: 0 });
});

test('fitView centres on the bounds and fits', () => {
  const b = { min_x: 0, max_x: 3, min_y: 0, max_y: 1 };
  const v = pure.fitView(b, 0, 256, 800, 600, -1, 2);
  assert.strictEqual(v.cx, 512);
  assert.strictEqual(v.cz, 256);
  assert.ok(v.scale <= 800 / 1024);
});

test('tileUrl and labels', () => {
  const e = { base: 'days/2026-10-22/0005', date: '2026-10-22', time: '0005' };
  assert.strictEqual(pure.tileUrl('../', e, 'nether', -1, 3, -2), '../days/2026-10-22/0005/render/tiles/nether/-1/3/-2.png');
  assert.strictEqual(pure.formatLabel(e), '2026-10-22 00:05');
  assert.deepStrictEqual(pure.dimsOf({ dimensions: { end: {}, overworld: {}, nether: {} } }), ['overworld', 'nether', 'end']);
});

/* ---------- DOM smoke test with a minimal stub ---------- */
function makeEnv() {
  const frames = [];
  const draws = [];
  const strokes = [];
  function makeEl(tag) {
    const handlers = {};
    const e = {
      tagName: tag.toUpperCase(), children: [], attrs: {}, style: {}, _text: '',
      classList: { add() {}, remove() {} }, className: '',
      appendChild(c) { this.children.push(c); return c; },
      setAttribute(k, v) { this.attrs[k] = v; },
      getAttribute(k) { return this.attrs[k]; },
      addEventListener(t, fn) { (handlers[t] = handlers[t] || []).push(fn); },
      fire(t, ev) { (handlers[t] || []).forEach((f) => f(Object.assign({ preventDefault() {}, target: e }, ev))); },
      getBoundingClientRect() { return { left: 0, top: 0, width: 800, height: 600 }; },
      setPointerCapture() {},
      getContext() {
        return {
          setTransform() {}, clearRect() {}, save() {}, restore() {}, fillRect() {},
          setLineDash() {}, drawImage(img) { draws.push(img.src); },
          strokeRect(x, y, w, h) { strokes.push([x, y, w, h, this.strokeStyle]); }
        };
      },
      get textContent() { return this._text; },
      set textContent(v) { this._text = v; if (v === '') this.children = []; },
      value: '', checked: false, disabled: false
    };
    return e;
  }
  const app = makeEl('div');
  const doc = {
    readyState: 'complete',
    createElement: makeEl,
    getElementById: (id) => (id === 'app' ? app : null),
    addEventListener() {}
  };
  class FakeImage {
    set src(v) { this._src = v; setTimeout(() => this.onload && this.onload(), 0); }
    get src() { return this._src; }
  }
  return { app, doc, frames, draws, strokes, FakeImage };
}

function loadViewer(data) {
  const env = makeEnv();
  global.window = global;
  global.document = env.doc;
  global.Image = env.FakeImage;
  global.devicePixelRatio = 1;
  global.requestAnimationFrame = (fn) => { env.frames.push(fn); return env.frames.length; };
  global.addEventListener = () => {};
  global.location = {};
  global.MAPVIEW = data;
  delete require.cache[require.resolve(VIEWER)];
  require(VIEWER);
  return env;
}

const dimMeta = (zmin, zmax) => ({
  tile_size: 256, zoom_min: zmin, zoom_max: zmax, tile_count: 4, region_count: 1, reused_previous: false,
  bounds: Object.fromEntries(Array.from({ length: zmax - zmin + 1 }, (_, i) => [String(zmin + i), { min_x: 0, max_x: 1, min_y: 0, max_y: 1 }]))
});

function sampleData(mode) {
  return {
    mode, prefix: mode === 'slider' ? '../' : '', title: 'Test',
    entries: [
      { date: '2026-10-21', time: '2300', base: 'days/2026-10-21/2300', utc_offset: '+02:00', snapshots: ['2300'],
        dimensions: { overworld: dimMeta(0, 0) }, changes: {} },
      { date: '2026-10-22', time: '1537', base: 'days/2026-10-22/1537', utc_offset: '+02:00', snapshots: ['0005', '1537'],
        dimensions: { overworld: dimMeta(0, 0), nether: dimMeta(0, 0) },
        changes: { overworld: { '0': [[1, 1, 'm'], [0, 0, 'a']] } } }
    ],
    siblings: []
  };
}

const flush = () => new Promise((r) => setTimeout(r, 5));
function runFrame(env) { const f = env.frames.splice(0); f.forEach((fn) => fn()); }
function find(node, pred, out = []) { if (pred(node)) out.push(node); (node.children || []).forEach((c) => find(c, pred, out)); return out; }

test('slider mode renders tiles of the latest day and tabs for its dimensions', async () => {
  const env = loadViewer(sampleData('slider'));
  runFrame(env);
  await flush();
  runFrame(env);
  const tabs = find(env.app, (n) => /^tab( on)?$/.test(n.className || ''));
  assert.strictEqual(tabs.length, 2);
  assert.ok(env.draws.length > 0, 'tiles drawn');
  assert.ok(env.draws.every((u) => u.startsWith('../days/2026-10-22/1537/render/tiles/overworld/0/')));
});

test('moving the slider back to a day without a nether hides that tab and keeps working', async () => {
  const env = loadViewer(sampleData('slider'));
  runFrame(env);
  global.__mapviewer.setEntry(0);
  const tabs = find(env.app, (n) => /^tab( on)?$/.test(n.className || ''));
  assert.strictEqual(tabs.length, 1);
  global.__mapviewer.setDim('nether'); // not available: must not throw
  runFrame(env);
});

test('show changes outlines changed tiles', async () => {
  const env = loadViewer(sampleData('slider'));
  runFrame(env);
  const box = find(env.app, (n) => n.attrs && n.attrs.type === 'checkbox')[0];
  box.checked = true;
  box.fire('change', {});
  runFrame(env);
  assert.strictEqual(env.strokes.length, 2);
  const colours = env.strokes.map((s) => s[4]).sort();
  assert.deepStrictEqual(colours, ['#2ecc71', '#f39c12']);
});

test('dragging pans the view and the coordinates readout updates', async () => {
  const env = loadViewer(sampleData('snapshot'));
  runFrame(env);
  const canvas = find(env.app, (n) => n.tagName === 'CANVAS')[0];
  const before = global.__mapviewer.state.view.cx;
  canvas.fire('pointerdown', { pointerId: 1, clientX: 100, clientY: 100 });
  canvas.fire('pointermove', { pointerId: 1, clientX: 140, clientY: 100 });
  assert.ok(global.__mapviewer.state.view.cx < before, 'dragging right moves the view left');
  const coords = find(env.app, (n) => n.className === 'coords')[0];
  assert.match(coords.textContent, /^X -?\d+\s+Z -?\d+$/);
});

test('wheel zoom changes scale', async () => {
  const env = loadViewer(sampleData('snapshot'));
  runFrame(env);
  const canvas = find(env.app, (n) => n.tagName === 'CANVAS')[0];
  const s0 = global.__mapviewer.state.view.scale;
  canvas.fire('wheel', { deltaY: -200, clientX: 400, clientY: 300 });
  assert.ok(global.__mapviewer.state.view.scale > s0);
});
