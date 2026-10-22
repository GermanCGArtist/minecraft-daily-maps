'use strict';
/* Control panel for Minecraft Daily Maps (Tauri 2, global API). */

(function () {
  var T = window.__TAURI__ || {};
  var DIMS = [
    { key: 'overworld', label: 'Overworld' },
    { key: 'nether', label: 'Nether' },
    { key: 'end', label: 'The End' }
  ];

  var S = null;          // last state from the backend
  var pending = [];      // dropped files waiting for confirmation
  var awaitingEdits = {}; // job id -> edited fields (inbox jobs)
  var rerenderTarget = null;
  var confirmResolve = null;

  function $(id) { return document.getElementById(id); }

  function h(tag, props, children) {
    var e = document.createElement(tag);
    Object.keys(props || {}).forEach(function (k) {
      var v = props[k];
      if (k === 'text') e.textContent = v;
      else if (k === 'class') e.className = v;
      else if (k === 'on') Object.keys(v).forEach(function (ev) { e.addEventListener(ev, v[ev]); });
      else if (k === 'checked' || k === 'disabled' || k === 'value') e[k] = v;
      else e.setAttribute(k, v);
    });
    (children || []).forEach(function (c) {
      if (c == null) return;
      e.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    });
    return e;
  }

  function call(cmd, args) {
    return T.core.invoke(cmd, args || {});
  }

  function toast(msg, kind) {
    var t = h('div', { class: 'toast' + (kind === 'info' ? ' info' : ''), text: String(msg) });
    $('toasts').appendChild(t);
    setTimeout(function () { t.remove(); }, kind === 'info' ? 3500 : 8000);
  }

  function fail(err) { toast(typeof err === 'string' ? err : (err && err.message) || String(err)); }

  /* ---------- helpers ---------- */

  function hhmm(t) { return t && t.length === 4 ? t.slice(0, 2) + ':' + t.slice(2) : t || ''; }
  function toInputTime(t) { return hhmm(t); }
  function fromInputTime(v) { return (v || '').replace(':', ''); }
  function fileName(p) { return p.split(/[\\/]/).pop(); }

  function dimChecks(selected, onChange) {
    var box = h('div', { class: 'checks' });
    DIMS.forEach(function (d) {
      var cb = h('input', { type: 'checkbox', checked: selected.indexOf(d.key) >= 0 });
      cb.addEventListener('change', function () { onChange(readChecks(box)); });
      box.appendChild(h('label', { class: 'check' }, [cb, d.label]));
      cb.dataset.dim = d.key;
    });
    return box;
  }

  function readChecks(box) {
    var out = [];
    box.querySelectorAll('input[type=checkbox]').forEach(function (cb) {
      if (cb.checked) out.push(cb.dataset.dim);
    });
    return out;
  }

  function importForm(model, onChange) {
    var date = h('input', { type: 'date', value: model.date });
    var time = h('input', { type: 'time', value: toInputTime(model.time) });
    date.addEventListener('input', function () { model.date = date.value; onChange && onChange(); });
    time.addEventListener('input', function () { model.time = fromInputTime(time.value); onChange && onChange(); });
    var dims = dimChecks(model.dimensions, function (v) { model.dimensions = v; onChange && onChange(); });
    return h('div', { class: 'form-grid' }, [
      h('span', { class: 'muted', text: 'Date' }), date,
      h('span', { class: 'muted', text: 'Time' }), time,
      h('span', { class: 'muted', text: 'Render' }), dims
    ]);
  }

  function validate(model) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(model.date || '')) return 'Pick a valid date.';
    if (!/^\d{4}$/.test(model.time || '')) return 'Pick a valid time.';
    if (!model.dimensions.length) return 'Select at least one dimension.';
    return null;
  }

  /* ---------- state ---------- */

  function refresh() {
    return call('get_state').then(function (s) { S = s; render(); }).catch(fail);
  }

  function applyState(s) { S = s; render(); }

  /* ---------- drop / pending ---------- */

  function handleFiles(paths) {
    var zips = (paths || []).filter(function (p) { return /\.zip$/i.test(p); });
    if (!zips.length) { toast('Only .zip world archives are supported.'); return; }
    if (!S || !S.data_dir) { toast('Choose a data folder first.'); return; }
    zips.forEach(function (p) {
      call('guess_time', { path: p }).then(function (g) {
        pending.push({
          path: p, name: fileName(p), date: g.date, time: g.time, source: g.source,
          dimensions: S.config.default_dimensions.slice()
        });
        renderPending();
      }).catch(fail);
    });
  }

  function renderPending() {
    var root = $('pending');
    root.textContent = '';
    pending.forEach(function (m, i) {
      var err = h('div', { class: 'msg error' });
      var card = h('div', { class: 'card' }, [
        h('div', { class: 'job-head' }, [
          h('div', { class: 'job-title', text: m.name }),
          h('span', { class: 'badge awaiting', text: 'confirm' })
        ]),
        h('div', { class: 'muted', text: m.source === 'filename' ? 'Date and time taken from the file name.' : 'No date in the file name: using the file’s modified time.' }),
        importForm(m),
        err,
        h('div', { class: 'actions' }, [
          h('button', { text: 'Cancel', on: { click: function () { pending.splice(i, 1); renderPending(); } } }),
          h('button', {
            class: 'primary', text: 'Add to queue',
            on: {
              click: function () {
                var v = validate(m);
                if (v) { err.textContent = v; return; }
                call('enqueue_import', { path: m.path, date: m.date, time: m.time, dimensions: m.dimensions })
                  .then(function () { pending.splice(pending.indexOf(m), 1); renderPending(); })
                  .catch(function (e) { err.textContent = String(e); });
              }
            }
          })
        ])
      ]);
      root.appendChild(card);
    });
  }

  /* ---------- queue ---------- */

  var STEP_ICON = { pending: '○', running: '↻', done: '✓', skip: '–', error: '✗' };

  function renderQueue() {
    var root = $('queue');
    root.textContent = '';
    var q = (S && S.queue) || [];
    if (!q.length) { root.appendChild(h('div', { class: 'empty', text: 'Nothing queued.' })); return; }
    q.slice().reverse().forEach(function (job) {
      var kids = [
        h('div', { class: 'job-head' }, [
          h('div', { class: 'job-title', text: job.title }),
          h('span', { class: 'badge ' + job.status, text: job.status })
        ])
      ];
      if (job.status === 'awaiting') {
        var model = awaitingEdits[job.id] || (awaitingEdits[job.id] = { date: job.date, time: job.time, dimensions: job.dimensions.slice() });
        var err = h('div', { class: 'msg error' });
        kids.push(h('div', { class: 'muted', text: 'Found in inbox. Check date and time, then confirm.' }));
        kids.push(importForm(model));
        kids.push(err);
        kids.push(h('div', { class: 'actions' }, [
          h('button', { text: 'Dismiss', on: { click: function () { call('dismiss_job', { id: job.id }).catch(fail); } } }),
          h('button', {
            class: 'primary', text: 'Confirm',
            on: {
              click: function () {
                var v = validate(model);
                if (v) { err.textContent = v; return; }
                call('confirm_job', { id: job.id, date: model.date, time: model.time, dimensions: model.dimensions })
                  .catch(function (e) { err.textContent = String(e); });
              }
            }
          })
        ]));
      } else {
        if (job.steps && job.steps.length) {
          kids.push(h('ul', { class: 'steps' }, job.steps.map(function (s) {
            return h('li', { class: s.state }, [
              h('span', { class: 'ico' + (s.state === 'running' ? ' spin' : ''), text: STEP_ICON[s.state] || '○' }),
              h('span', { text: s.label + (s.message ? ' — ' + s.message : '') })
            ]);
          })));
        }
        if (job.message) kids.push(h('div', { class: 'msg' + (job.status === 'error' ? ' error' : ''), text: job.message }));
        if (job.status !== 'running' && job.status !== 'queued') {
          kids.push(h('div', { class: 'actions' }, [
            h('button', { class: 'ghost', text: 'Dismiss', on: { click: function () { call('dismiss_job', { id: job.id }).catch(fail); } } })
          ]));
        }
      }
      root.appendChild(h('div', { class: 'job' }, kids));
    });
  }

  /* ---------- snapshots ---------- */

  function renderSnapshots() {
    var root = $('snapshots');
    root.textContent = '';
    var snaps = (S && S.snapshots) || [];
    $('snap-count').textContent = snaps.length ? snaps.length + (snaps.length === 1 ? ' snapshot' : ' snapshots') : '';
    if (!snaps.length) { root.appendChild(h('div', { class: 'empty', text: 'No snapshots yet. Drop a world zip to get started.' })); return; }
    var byDay = {};
    snaps.forEach(function (s) { (byDay[s.date] = byDay[s.date] || []).push(s); });
    Object.keys(byDay).sort().reverse().forEach(function (date) {
      var list = byDay[date].slice().sort(function (a, b) { return b.time.localeCompare(a.time); });
      root.appendChild(h('div', { class: 'day' }, [
        h('div', { class: 'day-head' }, [
          h('span', { text: date }),
          h('button', { class: 'ghost', text: 'Folder', on: { click: function () { call('open_folder', { date: date, time: null }).catch(fail); } } })
        ])
      ].concat(list.map(snapRow))));
    });
  }

  function snapRow(s) {
    var chips = (s.dimensions || []).map(function (d) { return h('span', { class: 'chip', text: d }); });
    if (!s.complete) chips.push(h('span', { class: 'chip warn', text: 'incomplete' }));
    if (s.tile_count) chips.push(h('span', { class: 'chip', text: s.tile_count.toLocaleString() + ' tiles' }));
    if (s.changed_tiles) chips.push(h('span', { class: 'chip', text: s.changed_tiles.toLocaleString() + ' changed' }));
    if (s.has_world) chips.push(h('span', { class: 'chip', text: 'world kept' }));
    return h('div', { class: 'snap' }, [
      h('div', { class: 'snap-time', text: hhmm(s.time) }),
      h('div', { class: 'chips' }, chips),
      h('div', { class: 'snap-actions' }, [
        h('button', { text: 'Open', disabled: !s.complete, on: { click: function () { call('open_snapshot', { date: s.date, time: s.time }).catch(fail); } } }),
        h('button', { text: 'Re-render', on: { click: function () { openRerender(s); } } }),
        h('button', { text: 'Folder', on: { click: function () { call('open_folder', { date: s.date, time: s.time }).catch(fail); } } }),
        h('button', {
          text: 'Delete',
          on: {
            click: function () {
              confirmDialog('Delete the snapshot ' + s.date + ' ' + hhmm(s.time) + ' including its zip, world and tiles? This cannot be undone.')
                .then(function (yes) {
                  if (!yes) return;
                  call('delete_snapshot', { date: s.date, time: s.time }).then(applyState).catch(fail);
                });
            }
          }
        })
      ])
    ]);
  }

  /* ---------- dialogs ---------- */

  function confirmDialog(text) {
    $('confirm-text').textContent = text;
    $('dlg-confirm').showModal();
    return new Promise(function (resolve) { confirmResolve = resolve; });
  }

  function closeConfirm(v) {
    $('dlg-confirm').close();
    if (confirmResolve) { confirmResolve(v); confirmResolve = null; }
  }

  function openRerender(s) {
    var selected = (s.dimensions && s.dimensions.length) ? s.dimensions.slice() : S.config.default_dimensions.slice();
    rerenderTarget = { date: s.date, time: s.time, dimensions: selected };
    $('rr-title').textContent = s.date + ' ' + hhmm(s.time) + (s.has_world ? '' : ' — the zip will be extracted again');
    var box = $('rr-dims');
    box.textContent = '';
    box.appendChild(dimChecks(selected, function (v) { rerenderTarget.dimensions = v; }));
    $('dlg-rerender').showModal();
  }

  function openSettings() {
    var c = S.config;
    $('set-platform').textContent = S.platform;
    $('set-unmined').value = c.unmined_paths[S.platform] || '';
    var dimsBox = $('set-dims');
    dimsBox.textContent = '';
    dimsBox.appendChild(dimChecks(c.default_dimensions, function () {}));
    $('set-zoom').value = c.zoom_in;
    $('set-keep').checked = !!c.keep_world;
    $('set-link').checked = !!c.link_unchanged_tiles;
    $('set-auto').checked = !!c.auto_import_inbox;
    $('set-extra').value = (c.extra_unmined_args || []).join(' ');
    $('set-datadir').textContent = S.data_dir || '';
    $('dlg-settings').showModal();
  }

  function pickFolder() {
    return T.dialog.open({ directory: true, multiple: false });
  }

  function saveSettings() {
    var c = JSON.parse(JSON.stringify(S.config));
    c.unmined_paths[S.platform] = $('set-unmined').value.trim();
    var dims = readChecks($('set-dims'));
    c.default_dimensions = dims.length ? dims : ['overworld'];
    c.zoom_in = Math.max(0, Math.min(4, parseInt($('set-zoom').value, 10) || 0));
    c.keep_world = $('set-keep').checked;
    c.link_unchanged_tiles = $('set-link').checked;
    c.auto_import_inbox = $('set-auto').checked;
    var extra = $('set-extra').value.trim();
    c.extra_unmined_args = extra ? extra.split(/\s+/) : [];
    return call('save_config', { config: c }).then(applyState);
  }

  /* ---------- render ---------- */

  function render() {
    if (!S) return;
    renderPending();
    renderQueue();
    renderSnapshots();
    if (!S.data_dir && !$('dlg-first').open) {
      $('first-path').value = S.suggested_data_dir;
      $('dlg-first').showModal();
    }
  }

  /* ---------- wiring ---------- */

  function wire() {
    $('btn-viewer').addEventListener('click', function () { call('open_viewer').catch(fail); });
    $('btn-folder').addEventListener('click', function () { call('open_folder', { date: null, time: null }).catch(fail); });
    $('btn-settings').addEventListener('click', function () { if (S && S.data_dir) openSettings(); });
    $('btn-clear').addEventListener('click', function () { call('clear_finished').catch(fail); });
    $('btn-browse').addEventListener('click', function () {
      T.dialog.open({ multiple: true, filters: [{ name: 'World zip', extensions: ['zip'] }] })
        .then(function (r) { if (r) handleFiles(Array.isArray(r) ? r : [r]); })
        .catch(fail);
    });

    $('first-browse').addEventListener('click', function () {
      pickFolder().then(function (p) { if (p) $('first-path').value = p; }).catch(fail);
    });
    $('form-first').addEventListener('submit', function (ev) {
      ev.preventDefault();
      var p = $('first-path').value.trim();
      if (!p) return;
      call('set_data_dir', { path: p }).then(function (s) { applyState(s); $('dlg-first').close(); openSettings(); }).catch(fail);
    });
    $('dlg-first').addEventListener('cancel', function (ev) { ev.preventDefault(); });

    $('set-browse').addEventListener('click', function () {
      T.dialog.open({ multiple: false, directory: false }).then(function (p) { if (p) $('set-unmined').value = p; }).catch(fail);
    });
    $('set-change-dir').addEventListener('click', function () {
      pickFolder().then(function (p) {
        if (!p) return;
        return call('set_data_dir', { path: p }).then(function (s) { applyState(s); $('set-datadir').textContent = s.data_dir; });
      }).catch(fail);
    });
    $('set-rebuild').addEventListener('click', function () {
      call('rebuild_site').then(function () { toast('HTML rebuilt.', 'info'); }).catch(fail);
    });
    $('set-cancel').addEventListener('click', function () { $('dlg-settings').close(); });
    $('form-settings').addEventListener('submit', function (ev) {
      ev.preventDefault();
      saveSettings().then(function () { $('dlg-settings').close(); toast('Settings saved.', 'info'); }).catch(fail);
    });

    $('rr-cancel').addEventListener('click', function () { $('dlg-rerender').close(); });
    $('form-rerender').addEventListener('submit', function (ev) {
      ev.preventDefault();
      if (!rerenderTarget.dimensions.length) { toast('Select at least one dimension.'); return; }
      call('enqueue_rerender', rerenderTarget).then(function () { $('dlg-rerender').close(); }).catch(fail);
    });

    $('confirm-yes').addEventListener('click', function () { closeConfirm(true); });
    $('confirm-no').addEventListener('click', function () { closeConfirm(false); });
    $('dlg-confirm').addEventListener('cancel', function () { if (confirmResolve) { confirmResolve(false); confirmResolve = null; } });

    // Real drag and drop (Tauri delivers file system paths).
    var zone = $('dropzone');
    if (T.webview && T.webview.getCurrentWebview) {
      T.webview.getCurrentWebview().onDragDropEvent(function (ev) {
        var p = ev.payload;
        if (p.type === 'enter' || p.type === 'over') zone.classList.add('over');
        else if (p.type === 'leave') zone.classList.remove('over');
        else if (p.type === 'drop') { zone.classList.remove('over'); handleFiles(p.paths); }
      });
    }

    T.event.listen('queue-update', function (ev) {
      if (!S) return;
      S.queue = ev.payload;
      renderQueue();
    });
    T.event.listen('snapshots-changed', function () { refresh(); });
  }

  if (!T.core) {
    document.body.insertBefore(h('p', { class: 'toast', text: 'This page must run inside the Minecraft Daily Maps desktop app.' }), document.body.firstChild);
    return;
  }
  wire();
  refresh();
})();
