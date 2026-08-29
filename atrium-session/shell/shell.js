// Atrium shell — the lifeless chrome.
//
// No behaviour yet. What is here is what the chrome needs to look like
// itself: who is logged in (from the session) and a launcher grid with
// placeholder applets, so the shape can be judged with something in it.
// Everything real — the state mirror, the window tree, launching — arrives
// section by section.

const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

// The catalogue: what the session found installed. Flat — grouping is a
// category string the shell may use later, not structure.
const apps = { all: [], filter: '' };

// Pins: which apps sit in the dock. Per browser for now (localStorage);
// they move into the session's state once the state mirror exists.
const pins = {
  KEY: 'atrium.pins',
  ids: [],
  load() { try { this.ids = JSON.parse(localStorage.getItem(this.KEY) || '[]'); } catch { this.ids = []; } },
  save() { try { localStorage.setItem(this.KEY, JSON.stringify(this.ids)); } catch {} },
  has(id) { return this.ids.includes(id); },
  toggle(id) { this.has(id) ? this.ids = this.ids.filter((x) => x !== id) : this.ids.push(id); this.save(); },
};

function iconFor(app, cls) {
  const icon = el('div', cls);
  if (app.icon) {
    const img = document.createElement('img'); img.src = app.icon; img.alt = '';
    icon.append(img);
  } else {
    icon.textContent = app.name.slice(0, 1).toUpperCase();
    icon.style.background = app.color || 'var(--accent)';
  }
  return icon;
}

function matches(app, q) {
  if (!q) return true;
  const hay = `${app.name} ${app.description} ${app.category || ''} ${app.id}`.toLowerCase();
  return q.split(/\s+/).filter(Boolean).every((w) => hay.includes(w));
}

function renderGrid() {
  const root = $('apps');
  root.replaceChildren();
  const shown = apps.all.filter((a) => matches(a, apps.filter.toLowerCase()));
  const cards = el('div', 'cards');
  for (const app of shown) {
    const card = el('div', 'card' + (pins.has(app.id) ? ' pinned' : ''));
    card.dataset.id = app.id;
    card.title = app.category ? `${app.name} · ${app.category}` : app.name;
    card.append(iconFor(app, 'icon'), el('div', 'title', app.name), el('div', 'blurb', app.description));
    card.addEventListener('contextmenu', (e) => { e.preventDefault(); pins.toggle(app.id); renderGrid(); renderDock(); });
    cards.append(card);
  }
  if (shown.length) root.append(cards);
  else root.append(el('div', 'empty-note', apps.all.length ? 'Nothing matches.' : 'No apps are installed. Packages ship them under /usr/share/atrium/apps.'));
  $('shown').textContent = `${shown.length} shown`;
  const cats = new Set(apps.all.map((a) => a.category).filter(Boolean));
  $('toolbox-sub').textContent = apps.all.length
    ? `${apps.all.length} app${apps.all.length === 1 ? '' : 's'}${cats.size ? ` across ${cats.size} categor${cats.size === 1 ? 'y' : 'ies'}` : ''} · right-click an app to pin it to the dock`
    : 'No apps installed';
  $('chip-pinned').textContent = `◇ Pinned · ${pins.ids.length}`;
}

function renderDock() {
  const root = $('pinned');
  root.replaceChildren();
  const byId = new Map(apps.all.map((a) => [a.id, a]));
  for (const id of pins.ids) {
    const app = byId.get(id);
    if (!app) continue;
    const b = el('button', 'dock-item');
    b.title = app.name;
    b.append(iconFor(app, 'tile'));
    b.addEventListener('contextmenu', (e) => { e.preventDefault(); pins.toggle(id); renderGrid(); renderDock(); });
    root.append(b);
  }
  root.hidden = pins.ids.length === 0;
}

async function loadApps() {
  try {
    const r = await fetch('/api/apps');
    apps.all = r.ok ? await r.json() : [];
  } catch { apps.all = []; }
  renderGrid();
  renderDock();
}

const $ = (id) => document.getElementById(id);

// Identity: everything the chrome says about who and where comes from the
// session host, which is the user and knows.
async function whoami() {
  try {
    const r = await fetch('/api/whoami');
    if (!r.ok) return;
    const me = await r.json();
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

// Menus: one open at a time, closed by a click elsewhere or Escape.
function menu(button, panel) {
  const open = () => { panel.hidden = false; button.setAttribute('aria-expanded', 'true'); };
  const close = () => { panel.hidden = true; button.setAttribute('aria-expanded', 'false'); };
  button.addEventListener('click', (e) => { e.stopPropagation(); panel.hidden ? open() : close(); });
  panel.addEventListener('click', (e) => e.stopPropagation());
  document.addEventListener('click', close);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
}

// Theme: per browser, remembered. Three states — light, dark, or follow the
// system — cycled by the button; the root attribute is what the CSS reads.
function theme() {
  const KEY = 'atrium.theme';
  const apply = (t) => {
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
    else delete document.documentElement.dataset.theme;
    $('theme').title = 'Theme: ' + (t || 'system');
  };
  let current = null;
  try { current = localStorage.getItem(KEY); } catch {}
  apply(current);
  $('theme').addEventListener('click', () => {
    current = current === 'light' ? 'dark' : current === 'dark' ? null : 'light';
    try { current ? localStorage.setItem(KEY, current) : localStorage.removeItem(KEY); } catch {}
    apply(current);
  });
}

pins.load();
$('filter').addEventListener('input', (e) => { apps.filter = e.target.value; renderGrid(); });
document.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); $('filter').focus(); $('filter').select(); }
});
loadApps();
menu($('avatar'), $('profile-menu'));
theme();
whoami();
