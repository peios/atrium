// Atrium shell.
//
// What is here is what the chrome can do today: identity from the session,
// the app catalogue in the Toolbox with search and pinning, the sidebar,
// theme. Everything real — the state mirror, the window tree, launching —
// arrives section by section.

import { icon } from '/shell/icons.js';

const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

// Static icons declared in the markup: <span data-icon="name" data-size="16">.
for (const holder of document.querySelectorAll('[data-icon]')) {
  holder.replaceChildren(icon(holder.dataset.icon, Number(holder.dataset.size || 16)));
}

// ---- Identity ---------------------------------------------------------
// Everything the chrome says about who and where comes from the session
// host, which is the user and knows.
// Filled by whoami(); the app bus hands it to apps at handshake.
let me = {};

async function whoami() {
  try {
    const r = await fetch('/api/whoami');
    if (!r.ok) return;
    me = await r.json();
    const name = me.display_name || me.user || '?';
    $('username').textContent = me.user || '?';
    if (me.hostname) $('hostname').textContent = me.hostname;
    $('avatar').textContent = initials(name);
    $('menu-name').textContent = name;
    $('menu-user').textContent = me.user || '';
    $('menu-host').textContent = me.hostname || '';
    $('menu-session').textContent = me.session || me.logon_session || '';
    $('menu-sid').textContent = me.user_sid || '';
    $('menu-sid').title = me.user_sid || '';
  } catch { /* the chrome stands on its own */ }
}

function initials(name) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const s = parts.length >= 2 ? parts[0][0] + parts[parts.length - 1][0] : name.slice(0, 2);
  return s.toUpperCase();
}

// ---- Menus ------------------------------------------------------------
// One open at a time, closed by a click elsewhere or Escape.
function menu(button, panel) {
  const open = () => { panel.hidden = false; button.setAttribute('aria-expanded', 'true'); };
  const close = () => { panel.hidden = true; button.setAttribute('aria-expanded', 'false'); };
  button.addEventListener('click', (e) => { e.stopPropagation(); panel.hidden ? open() : close(); });
  panel.addEventListener('click', (e) => e.stopPropagation());
  document.addEventListener('click', close);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
}

// ---- Theme ------------------------------------------------------------
// Per browser, remembered. The button flips whatever is showing: the
// three-state cycle (light, dark, system) reads as a dead click whenever
// "system" happens to equal the state before it. Following the system is
// the default until the button is first used; making it choosable again
// belongs in Settings.
function theme() {
  const KEY = 'atrium.theme';
  const showingDark = () => document.documentElement.dataset.theme === 'dark'
    || (!document.documentElement.dataset.theme && matchMedia('(prefers-color-scheme: dark)').matches);
  const apply = (t) => {
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
    else delete document.documentElement.dataset.theme;
    $('theme').title = showingDark() ? 'Switch to light' : 'Switch to dark';
    $('theme').replaceChildren(icon(showingDark() ? 'sun' : 'moon'));
    bus.themeChanged(showingDark() ? 'dark' : 'light');
  };
  let stored = null;
  try { stored = localStorage.getItem(KEY); } catch {}
  apply(stored);
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => apply(document.documentElement.dataset.theme || null));
  $('theme').addEventListener('click', () => {
    const next = showingDark() ? 'light' : 'dark';
    try { localStorage.setItem(KEY, next); } catch {}
    apply(next);
  });
}

// ---- Sidebar ----------------------------------------------------------
// Collapsed (a dock of glyphs) or expanded (labels and groups). Per
// browser, remembered; `[` toggles it.
function sidebar() {
  const KEY = 'atrium.sidebar';
  const sb = $('sidebar');
  const apply = (collapsed) => {
    sb.classList.toggle('collapsed', collapsed);
    document.documentElement.dataset.sideCollapsed = collapsed ? '1' : '0';
    $('collapse').title = collapsed ? 'Expand sidebar' : 'Collapse sidebar';
    $('collapse').setAttribute('aria-label', $('collapse').title);
  };
  let collapsed = true;
  try { collapsed = localStorage.getItem(KEY) !== 'open'; } catch {}
  apply(collapsed);
  const toggle = () => { collapsed = !collapsed; try { localStorage.setItem(KEY, collapsed ? 'closed' : 'open'); } catch {} apply(collapsed); };
  $('collapse').addEventListener('click', toggle);
  document.addEventListener('keydown', (e) => {
    if (e.key !== '[' || e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    e.preventDefault();
    toggle();
  });
}

// ---- Apps -------------------------------------------------------------
// The catalogue: what the session found installed. Flat — grouping is a
// category string, not structure. Pins are per browser (localStorage)
// until the session's state mirror exists.
const apps = { all: [], filter: '', pinnedOnly: false };
const pins = {
  KEY: 'atrium.pins',
  ids: [],
  load() { try { this.ids = JSON.parse(localStorage.getItem(this.KEY) || '[]'); } catch { this.ids = []; } },
  save() { try { localStorage.setItem(this.KEY, JSON.stringify(this.ids)); } catch {} },
  has(id) { return this.ids.includes(id); },
  toggle(id) { this.has(id) ? this.ids = this.ids.filter((x) => x !== id) : this.ids.push(id); this.save(); },
};

/** The coloured chip with the app's glyph, in one of the design's sizes. */
function glyph(app, size) {
  const g = el('div', `tb-applet-glyph ${size}`);
  g.style.background = app.color || 'oklch(58% 0.16 250)';
  const px = { lg: 22, md: 16, sm: 12, xs: 10 }[size] || 16;
  if (app.icon) {
    const img = document.createElement('img'); img.src = app.icon; img.alt = '';
    g.append(img);
  } else if (app.glyph) {
    g.append(icon(app.glyph, px, 1.8));
  } else {
    g.append(el('span', 'letter', app.name.slice(0, 1).toUpperCase()));
  }
  return g;
}

function matches(app, q) {
  if (!q) return true;
  const hay = `${app.name} ${app.description} ${app.category || ''} ${app.id}`.toLowerCase();
  return q.split(/\s+/).filter(Boolean).every((w) => hay.includes(w));
}

function visible() {
  const q = apps.filter.trim().toLowerCase();
  return apps.all.filter((a) => matches(a, q) && (!apps.pinnedOnly || pins.has(a.id)));
}

function renderGrid() {
  const root = $('apps');
  root.replaceChildren();
  const shown = visible();
  if (shown.length) {
    const grid = el('div', 'tbA-grid');
    for (const app of shown) {
      const tile = el('button', 'tbA-tile');
      tile.type = 'button';
      tile.dataset.id = app.id;
      if (session.isOpen(app.id)) tile.classList.add('is-open');
      tile.title = 'Open — right-click to pin';
      if (pins.has(app.id)) {
        const pc = el('span', 'pin-corner'); pc.title = 'Pinned'; pc.append(icon('check', 11));
        tile.append(pc);
      }
      const more = el('button', 'tbU-more tbU-more-tile'); more.type = 'button';
      more.setAttribute('aria-label', `More options for ${app.name}`);
      more.append(icon('more', 14));
      more.addEventListener('click', (e) => {
        e.stopPropagation();
        const r = e.currentTarget.getBoundingClientRect();
        openCtx(app, r.right - 8, r.bottom + 4);
      });
      tile.append(more, glyph(app, 'lg'), el('div', 'name', app.name), el('div', 'summary', app.description));
      tile.addEventListener('click', () => session.launch(app.id));
      tile.addEventListener('contextmenu', (e) => { e.preventDefault(); openCtx(app, e.clientX, e.clientY); });
      grid.append(tile);
    }
    root.append(grid);
  } else {
    const empty = el('div', 'tbU-empty');
    if (!apps.all.length) empty.textContent = 'No apps are installed. Packages ship them under /usr/share/atrium/apps.';
    else if (apps.filter.trim()) { empty.append('No apps match ', el('span', 'mono', `“${apps.filter.trim()}”`), '.'); }
    else empty.textContent = 'Nothing is pinned yet. Right-click an app to pin it.';
    root.append(empty);
  }
  $('shown').textContent = apps.filter.trim() ? `${shown.length} match${shown.length === 1 ? '' : 'es'}` : `${shown.length} shown`;
  const cats = new Set(apps.all.map((a) => a.category).filter(Boolean));
  $('toolbox-sub').textContent = apps.all.length
    ? `${apps.all.length} app${apps.all.length === 1 ? '' : 's'}${cats.size ? ` across ${cats.size} categor${cats.size === 1 ? 'y' : 'ies'}` : ''} · right-click any app to pin it to the sidebar`
    : 'No apps installed';
  $('nav-toolbox-count').textContent = apps.all.length;
  $('chip-pinned-n').textContent = `· ${pins.ids.length}`;
  $('chip-pinned').classList.toggle('on', apps.pinnedOnly);
  $('filter-clear').hidden = !apps.filter;
}

// Pinned apps grouped by category, each with a running dot when it has a
// window; then open-but-unpinned apps under "Open", Ubuntu-dock style.
function renderSidebar() {
  const root = $('nav-pinned');
  root.replaceChildren();
  const byId = new Map(apps.all.map((a) => [a.id, a]));
  const pinned = pins.ids.map((id) => byId.get(id)).filter(Boolean);
  const windowsOf = (appId) => [...session.windows.values()].map(({ window: w }) => w).filter((w) => w.app === appId);
  // One entry per open window (its title, a running dot, dimmed when
  // minimised); an app with no window gets a single launcher entry.
  const windowItem = (app, w, unpinHint) => {
    const active = w.id === session.focus && session.view === 'windows';
    const b = el('button', 'nav-item is-open' + (active ? ' active' : '') + (w.minimized ? ' is-min' : ''));
    b.type = 'button';
    b.title = `${w.title}${w.minimized ? ' — minimised' : ''}${unpinHint ? ' — right-click for options' : ''}`;
    b.append(glyph(app, 'xs'), el('span', 'nav-label', w.title));
    const dot = el('span', 'nav-running'); dot.setAttribute('aria-label', 'open'); b.append(dot);
    b.addEventListener('click', () => session.focusWindow(w.id));
    b.addEventListener('auxclick', (e) => { if (e.button === 1) session.close(w.id); });
    b.addEventListener('contextmenu', (e) => { e.preventDefault(); openCtx(app, e.clientX, e.clientY, w); });
    return b;
  };
  const launcherItem = (app) => {
    const b = el('button', 'nav-item'); b.type = 'button';
    b.title = `${app.name} — right-click to unpin`;
    b.append(glyph(app, 'xs'), el('span', 'nav-label', app.name));
    b.addEventListener('click', () => session.launch(app.id));
    b.addEventListener('contextmenu', (e) => { e.preventDefault(); openCtx(app, e.clientX, e.clientY); });
    return b;
  };
  const item = (app, unpinHint) => {
    const ws = windowsOf(app.id);
    if (!ws.length) return launcherItem(app);
    const frag = document.createDocumentFragment();
    for (const w of ws) frag.append(windowItem(app, w, unpinHint));
    return frag;
  };
  if (pinned.length) {
    // Grouped by category, in first-seen order; uncategorised apps last.
    const groups = new Map();
    for (const a of pinned) {
      const cat = a.category || 'Other';
      if (!groups.has(cat)) groups.set(cat, []);
      groups.get(cat).push(a);
    }
    for (const [cat, items] of groups) {
      const g = el('div', 'nav-group');
      g.append(el('div', 'nav-group-label', cat));
      for (const app of items) g.append(item(app, true));
      root.append(g);
    }
  }
  const openUnpinned = [...new Set([...session.windows.values()].map(({ window: w }) => w.app))]
    .filter((id) => !pins.has(id)).map((id) => byId.get(id)).filter(Boolean);
  if (openUnpinned.length) {
    const g = el('div', 'nav-group');
    g.append(el('div', 'nav-group-label', 'Open'));
    for (const app of openUnpinned) g.append(item(app, false));
    root.append(g);
  }
  if (!pinned.length && !openUnpinned.length) {
    root.append(el('div', 'sidebar-empty', 'Nothing pinned yet — right-click an app in the Toolbox.'));
  }
}

function render() { renderGrid(); renderSidebar(); }

// ---- Context menu -----------------------------------------------------
function openCtx(app, x, y, win) {
  const ctx = $('ctx');
  ctx.replaceChildren();
  const meta = el('div', 'tb-ctx-meta');
  meta.append(el('div', 'name', win ? win.title : app.name), el('div', 'from', app.category ? `from ${app.category}` : app.id));
  const item = (glyphName, label, kbd, onClick, dim) => {
    const b = el('button', 'tb-ctx-item' + (dim ? ' dim' : '')); b.type = 'button';
    b.append(icon(glyphName, 12), el('span', '', label));
    if (kbd) b.append(el('span', 'kbd', kbd));
    b.addEventListener('click', () => { closeCtx(); if (onClick) onClick(); });
    return b;
  };
  const pinned = pins.has(app.id);
  ctx.append(meta);
  if (win) {
    ctx.append(
      item('play', win.minimized ? 'Restore' : 'Switch to', '↵', () => session.focusWindow(win.id)),
      ...(win.minimized ? [] : [item('minus', 'Minimise', null, () => session.minimize(win.id))]),
      item('close', 'Close window', null, () => session.close(win.id)),
    );
  } else {
    ctx.append(
      item('play', session.isOpen(app.id) ? 'Switch to' : 'Open', '↵', () => session.launch(app.id)),
    );
  }
  ctx.append(
    item('plus', 'Open in new window', null, () => session.launch(app.id, true)),
    el('div', 'tb-ctx-sep'),
    item(pinned ? 'close' : 'plus', pinned ? 'Unpin from sidebar' : 'Pin to sidebar', null, () => { pins.toggle(app.id); render(); }),
    item('info', 'About this app…', null, null, true),
  );
  ctx.hidden = false;
  const W = 240, H = 180;
  ctx.style.left = Math.min(x, window.innerWidth - W - 8) + 'px';
  ctx.style.top = Math.min(y, window.innerHeight - H - 8) + 'px';
}
function closeCtx() { $('ctx').hidden = true; }
document.addEventListener('mousedown', (e) => { if (!$('ctx').contains(e.target)) closeCtx(); });
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeCtx(); });
$('ctx').addEventListener('contextmenu', (e) => e.preventDefault());

async function loadApps() {
  try {
    const r = await fetch('/api/apps');
    apps.all = r.ok ? await r.json() : [];
  } catch { apps.all = []; }
  render();
}

// ---- Command bar hint -------------------------------------------------
// The design's rotating "try …" hint. Purely decorative until the
// palette exists; ⌘K focuses the Toolbox search meanwhile.
function cmdk() {
  const hints = ['Open terminal', 'Find service…', 'View events', 'Pin an app', 'Log out'];
  let i = 0;
  const rot = $('cmdk-rot'), hint = $('cmdk-hint');
  setInterval(() => {
    rot.classList.remove('in'); rot.classList.add('out');
    setTimeout(() => { i = (i + 1) % hints.length; hint.textContent = `"${hints[i]}"`; rot.classList.remove('out'); rot.classList.add('in'); }, 300);
  }, 2900);
  // Ctrl+K itself lives in the session's keymap (action "command-bar").
  $('cmdk').addEventListener('click', () => { showToolbox(); $('filter').focus(); $('filter').select(); });
}

// ---- Session mirror ---------------------------------------------------
// The session owns the windows; every tab is a mirror of them. On connect
// we get a snapshot, then events. Frames are created once per window and
// only ever shown/hidden — re-parenting an iframe reloads the app.
const session = {
  sock: null,
  windows: new Map(),   // id -> { window, frame, booted }
  focus: null,
  view: 'toolbox',      // this tab's choice: 'toolbox' | 'windows'
  queue: [],            // sends made while the socket was still connecting
  keymap: [],           // [{chord, action}] — the session's chord table
  send(msg) {
    if (this.sock && this.sock.readyState === 1) this.sock.send(JSON.stringify(msg));
    else {
      // Not connected (yet). Queue rather than drop: a click during the
      // first half-second, or mid-reconnect, should simply happen when
      // the socket is back.
      console.info('session: queued until connected:', msg.t);
      this.queue.push(msg);
    }
  },
  flush() {
    const q = this.queue.splice(0);
    for (const msg of q) this.send(msg);
  },
  // Focuses the app's existing window unless `fresh`; the session decides.
  launch(appId, fresh = false) { this.view = 'windows'; this.send({ t: 'launch', app: appId, new: fresh }); },
  isOpen(appId) { for (const { window: w } of this.windows.values()) if (w.app === appId) return true; return false; },
  focusWindow(id) { this.view = 'windows'; this.send({ t: 'focus', id }); },
  minimize(id) { this.send({ t: 'minimize', id }); },
  close(id) { this.send({ t: 'close', id }); },
};

let wsFailures = 0;

// A websocket that will not connect can mean the session itself is gone —
// a stale cookie after the box rebooted, a logged-out session. Ask over
// plain HTTP: if the answer is not "you're logged in", reload, which lands
// on the login page and starts clean.
async function checkSessionAlive() {
  try {
    const r = await fetch('/api/whoami', { cache: 'no-store' });
    if (!r.ok || r.redirected) location.replace('/');
  } catch { /* box unreachable; keep retrying the socket */ }
}

function connectSession() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  const sock = new WebSocket(`${proto}://${location.host}/ws`);
  session.sock = sock;
  sock.addEventListener('message', (e) => {
    let m; try { m = JSON.parse(e.data); } catch { return; }
    switch (m.t) {
      case 'snapshot': applySnapshot(m); break;
      case 'window.opened': addWindow(m.window); session.view = 'windows'; renderWindows(); break;
      case 'window.closed': removeWindow(m.id); renderWindows(); break;
      case 'window.minimized': { const e = session.windows.get(m.id); if (e) e.window.minimized = true; renderWindows(); break; }
      case 'window.restored': { const e = session.windows.get(m.id); if (e) e.window.minimized = false; renderWindows(); break; }
      case 'window.titled': { const e = session.windows.get(m.id); if (e) { e.window.title = m.title; e.frame.title = m.title; } renderWindows(); break; }
      case 'app.reply': bus.deliverReply(m); break;
      case 'app.stream': bus.deliverStream(m); break;
      case 'focus': session.focus = m.id; session.view = m.id !== null ? 'windows' : 'toolbox'; renderWindows(); break;
      case 'error': console.warn('session:', m.message); break;
    }
  });
  sock.addEventListener('open', () => { console.info('session: connected'); wsFailures = 0; session.flush(); });
  sock.addEventListener('error', () => console.warn('session: websocket error'));
  sock.addEventListener('close', (e) => {
    session.sock = null;
    wsFailures += 1;
    // Fast first retries, backing off to 3s; every third failure, check
    // whether this session still exists at all.
    const delay = Math.min(300 * wsFailures, 3000);
    console.warn(`session: websocket closed (code ${e.code}${e.reason ? `, ${e.reason}` : ''}); retry in ${delay}ms`);
    if (wsFailures % 3 === 0) checkSessionAlive();
    setTimeout(connectSession, delay);
  });
}

// Coming back to the tab, or the network coming back, is the moment to
// stop waiting out a backoff.
for (const evt of ['visibilitychange', 'online', 'focus']) {
  addEventListener(evt, () => {
    if (document.visibilityState === 'hidden') return;
    if (!session.sock || session.sock.readyState === 3) connectSession();
  });
}

function applySnapshot(m) {
  session.keymap = m.keymap || [];
  // Diff against what we have: keep live frames, drop gone ones, add new.
  const keep = new Set(m.windows.map((w) => w.id));
  for (const id of [...session.windows.keys()]) if (!keep.has(id)) removeWindow(id);
  for (const w of m.windows) {
    if (!session.windows.has(w.id)) addWindow(w);
    else session.windows.get(w.id).window = w;
  }
  session.focus = m.focus;
  if (session.focus === null) session.view = 'toolbox';
  renderWindows();
}

function addWindow(w) {
  const frame = document.createElement('iframe');
  frame.src = w.url;
  frame.title = w.title;
  frame.dataset.id = w.id;
  frame.hidden = true;
  $('ws-frames').append(frame);
  // `booted` gates visibility: set by the SDK handshake, or by a grace
  // timer so an app that never loads the SDK still shows.
  const entry = { window: w, frame, booted: false };
  entry.graceTimer = setTimeout(() => { entry.booted = true; renderWindows(); }, 1200);
  session.windows.set(w.id, entry);
}

function removeWindow(id) {
  const entry = session.windows.get(id);
  if (!entry) return;
  entry.frame.remove();
  session.windows.delete(id);
}

function showToolbox() { session.view = 'toolbox'; renderWindows(); }

function renderWindows() {
  renderSidebar();
  for (const tile of document.querySelectorAll('.tbA-tile')) tile.classList.toggle('is-open', session.isOpen(tile.dataset.id));
  // Windows show only when something is focused and visible; otherwise
  // the Toolbox, chrome and all.
  const focusedEntry = session.focus !== null ? session.windows.get(session.focus) : null;
  const showWs = session.view === 'windows' && !!focusedEntry && !focusedEntry.window.minimized;
  $('ws').hidden = !showWs;
  $('toolbox').hidden = showWs;
  $('nav-toolbox').classList.toggle('is-active', !showWs);
  // The bar shows the focused window; frames are only toggled.
  const focused = session.focus !== null ? session.windows.get(session.focus) : null;
  for (const [id, entry] of session.windows) entry.frame.hidden = !(showWs && id === session.focus && entry.booted);
  const title = $('win-title');
  title.replaceChildren();
  if (focused) {
    const app = apps.all.find((a) => a.id === focused.window.app);
    if (app) title.append(glyph(app, 'xs'));
    title.append(el('span', '', focused.window.title));
  }
}

$('win-close').addEventListener('click', () => { if (session.focus !== null) session.close(session.focus); });
$('win-min').addEventListener('click', () => { if (session.focus !== null) session.minimize(session.focus); });

// ---- Keybinds ---------------------------------------------------------
// The session owns the table (it arrives in the snapshot); the shell owns
// the actions. While an app frame has focus the SDK forwards reserved
// chords back here — see the bus below. Chord form: sorted modifiers +
// key, "Ctrl+Alt+T"; letters uppercased, so layouts follow the keyboard.
function chordOf(e) {
  const mods = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.metaKey) mods.push('Meta');
  if (e.shiftKey) mods.push('Shift');
  let key = e.key;
  if (key === ' ') key = 'Space';
  if (key.length === 1) key = key.toUpperCase();
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(key)) return null;
  return [...mods, key].join('+');
}

function visibleWindows() {
  return [...session.windows.values()].map(({ window: w }) => w).filter((w) => !w.minimized);
}

function runAction(action) {
  const m = action.match(/^focus-window-([1-9])$/);
  if (m) {
    const w = [...session.windows.values()][Number(m[1]) - 1];
    if (w) session.focusWindow(w.window.id);
    return true;
  }
  switch (action) {
    case 'command-bar':
      showToolbox();
      $('filter').focus();
      $('filter').select();
      return true;
    case 'toolbox':
      showToolbox();
      return true;
    case 'close-window':
      if (session.focus !== null && session.view === 'windows') session.close(session.focus);
      return true;
    case 'minimize-window':
      if (session.focus !== null && session.view === 'windows') session.minimize(session.focus);
      return true;
    case 'cycle-window': {
      const vis = visibleWindows();
      if (!vis.length) return true;
      const at = vis.findIndex((w) => w.id === session.focus);
      session.focusWindow(vis[(at + 1) % vis.length].id);
      return true;
    }
    default:
      console.warn('keybind: unknown action', action);
      return false;
  }
}

window.addEventListener('keydown', (e) => {
  const chord = chordOf(e);
  if (!chord) return;
  const hit = session.keymap.find((b) => b.chord === chord);
  if (!hit) return;
  e.preventDefault();
  e.stopPropagation();
  runAction(hit.action);
}, true);

// ---- App bus ----------------------------------------------------------
// The shell's half of the SDK: apps postMessage here, the shell vouches
// for which window each frame is and relays what needs the session. The
// frame is identified by its contentWindow — an app cannot claim to be a
// window it is not in.
const bus = {
  frameWindow(source) {
    for (const [id, entry] of session.windows) {
      if (entry.frame.contentWindow === source) return { id, entry };
    }
    return null;
  },
  themeChanged(theme) {
    for (const { frame } of session.windows.values()) {
      if (frame.contentWindow) frame.contentWindow.postMessage({ t: 'theme', theme }, '*');
    }
  },
  deliverStream(m) {
    const entry = session.windows.get(m.win);
    if (entry && entry.frame.contentWindow) entry.frame.contentWindow.postMessage({ t: 'stream', req: m.req, body: m.body }, '*');
  },
  deliverReply(m) {
    const entry = session.windows.get(m.win);
    if (!entry || !entry.frame.contentWindow) return;
    const { body } = m;
    const out = body && body.error ? { t: 'reply', req: m.req, error: body.error } : { t: 'reply', req: m.req, body };
    entry.frame.contentWindow.postMessage(out, '*');
  },
};

window.addEventListener('message', (e) => {
  const m = e.data;
  if (!m || typeof m.t !== 'string') return;
  const hit = bus.frameWindow(e.source);
  if (!hit) return; // not one of our frames
  const { id, entry } = hit;
  switch (m.t) {
    case 'hello': {
      clearTimeout(entry.graceTimer);
      entry.booted = true;
      const theme = document.documentElement.dataset.theme
        || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
      e.source.postMessage({
        t: 'ready',
        window: id,
        theme,
        user: { user: me.user || '', display_name: me.display_name || '' },
        reserved: session.keymap.map((b) => b.chord),
      }, '*');
      renderWindows();
      break;
    }
    case 'set-title':
      if (typeof m.title === 'string') session.send({ t: 'set-title', id, title: m.title });
      break;
    case 'close':
      session.close(id);
      break;
    case 'request':
      if (Number.isInteger(m.req)) session.send({ t: 'app', win: id, req: m.req, body: m.body ?? {} });
      break;
    case 'chord': {
      // A reserved chord captured inside the app's frame. Trust the table,
      // not the message: only chords in the keymap do anything.
      const hit = session.keymap.find((b) => b.chord === m.chord);
      if (hit) runAction(hit.action);
      break;
    }
  }
});

// ---- Wire up ----------------------------------------------------------
pins.load();
$('filter').addEventListener('input', (e) => { apps.filter = e.target.value; renderGrid(); });
$('filter-clear').addEventListener('click', () => { $('filter').value = ''; apps.filter = ''; renderGrid(); $('filter').focus(); });
$('chip-pinned').addEventListener('click', () => { apps.pinnedOnly = !apps.pinnedOnly; renderGrid(); });
menu($('avatar'), $('profile-menu'));
theme();
sidebar();
cmdk();
$('nav-toolbox').addEventListener('click', showToolbox);
window.atrium = { session };   // for the console and the dev harness
loadApps().then(connectSession);
whoami();
