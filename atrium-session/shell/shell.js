// Atrium shell — the lifeless chrome.
//
// No behaviour yet. What is here is what the chrome needs to look like
// itself: who is logged in (from the session) and a launcher grid with
// placeholder applets, so the shape can be judged with something in it.
// Everything real — the state mirror, the window tree, launching — arrives
// section by section.

const PLACEHOLDER = [
  { name: 'Peios', kind: 'FIRST-PARTY', desc: 'The platform itself', color: '#2f6fed', applets: [
    ['Dashboard', 'Machine status, alerts, at-a-glance health.', '#3b82f6'],
    ['Events', 'Audit log, kernel events, security trail.', '#3b82f6'],
    ['Updates', 'OS and app update channels, staged rollouts.', '#3b82f6'],
    ['Settings', 'Machine settings, time, locale.', '#3b82f6'],
    ['Terminal', 'Tabbed shell with splits and per-tab broadcast.', '#3b82f6'],
    ['Principals', 'Every user, group, service and machine.', '#a855f7'],
    ['Policies', 'Access policies, RBAC, conditional access.', '#a855f7'],
    ['Services', 'Long-running services: status, restart, logs.', '#22c55e'],
    ['Networking', 'Interfaces, routes, firewall, NAT.', '#06b6d4'],
    ['Registry', 'Layered configuration: base, local, overlays.', '#3b82f6'],
    ['Storage', 'Pools, volumes, SMART summaries.', '#22c55e'],
    ['Packages', 'Installed software, repositories, upgrades.', '#22c55e'],
  ]},
  { name: 'File Server', kind: 'APP', desc: 'SMB, NFS and FTP file sharing', color: '#f97316', ver: 'v0.0.0', applets: [
    ['Shares', 'Exported shares and who may see them.', '#f97316'],
    ['Files', 'Browse and manage files on this machine.', '#f97316'],
    ['Sessions', 'Open connections and locks.', '#f97316'],
  ]},
];

const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

function renderApps(root, apps) {
  root.replaceChildren();
  for (const app of apps) {
    const section = el('section', 'app-section');
    const head = el('div', 'app-head');
    const swatch = el('span', 'swatch'); swatch.style.background = app.color;
    head.append(swatch, el('span', 'name', app.name), el('span', 'kind', app.kind), el('span', 'desc', app.desc));
    if (app.ver) head.append(el('span', 'ver', app.ver));
    const cards = el('div', 'cards');
    for (const [title, blurb, color] of app.applets) {
      const card = el('div', 'card');
      const icon = el('div', 'icon', '▣'); icon.style.background = color;
      card.append(icon, el('div', 'title', title), el('div', 'blurb', blurb));
      cards.append(card);
    }
    section.append(head, cards);
    root.append(section);
  }
}

async function whoami() {
  try {
    const r = await fetch('/api/whoami');
    if (!r.ok) return;
    const me = await r.json();
    document.getElementById('username').textContent = me.user || '?';
    if (me.hostname) document.getElementById('hostname').textContent = me.hostname;
    document.getElementById('avatar').textContent = (me.user || '??').slice(0, 2).toUpperCase();
  } catch { /* the chrome stands on its own */ }
}

renderApps(document.getElementById('apps'), PLACEHOLDER);
whoami();
