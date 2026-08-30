// The Atrium SDK, v0 — the in-browser half of the app contract.
//
// An app runs in an iframe and talks to the shell with postMessage; the
// shell relays what needs the session over its own websocket. postMessage
// is the one channel that survives every isolation boundary Atrium may put
// around an app (foreign origins, sandboxed frames with no cookies), so it
// is the only thing this SDK assumes. Control flows here; bulk streams will
// get their own channel when an app needs one.
//
// Usage:
//   import { atrium } from '/shell/sdk.js';
//   const ctx = await atrium.ready();          // { window, theme, user }
//   atrium.on('theme', (t) => …);
//   atrium.setTitle('…'); atrium.close();
//   const reply = await atrium.request({ kind: 'system-info' });

const pending = new Map(); // req id -> { resolve, reject }
const handlers = new Map(); // event -> [fn]
const localShortcuts = new Map(); // chord -> [fn]
let reserved = [];
let seq = 0;
let context = null;
let announceReady;
const readyPromise = new Promise((resolve) => { announceReady = resolve; });

function emit(event, detail) {
  for (const fn of handlers.get(event) || []) {
    try { fn(detail); } catch (e) { console.error('atrium sdk handler:', e); }
  }
}

window.addEventListener('message', (e) => {
  if (e.source !== window.parent) return;
  const m = e.data;
  if (!m || typeof m.t !== 'string') return;
  switch (m.t) {
    case 'ready':
      context = { window: m.window, theme: m.theme, user: m.user };
      reserved = Array.isArray(m.reserved) ? m.reserved : [];
      document.documentElement.dataset.theme = m.theme;
      announceReady(context);
      break;
    case 'theme':
      if (context) context.theme = m.theme;
      document.documentElement.dataset.theme = m.theme;
      emit('theme', m.theme);
      break;
    case 'stream': {
      const p = pending.get(m.req);
      if (p && p.onStream) {
        try { p.onStream(m.body); } catch (e) { console.error('atrium sdk stream handler:', e); }
      }
      break;
    }
    case 'reply': {
      const p = pending.get(m.req);
      if (!p) return;
      pending.delete(m.req);
      if (m.error) p.reject(new Error(m.error));
      else if (m.body && m.body.error) p.reject(new Error(m.body.error));
      else p.resolve(m.body);
      break;
    }
  }
});

function post(msg) {
  window.parent.postMessage(msg, '*');
}

// The shell's chords work while this frame has focus: the shell told us
// which chords are reserved (with `ready`), we capture them before the app
// sees them and hand them back. Everything else is the app's — including
// anything registered with atrium.shortcuts.
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

window.addEventListener('keydown', (e) => {
  const chord = chordOf(e);
  if (!chord) return;
  if (reserved.includes(chord)) {
    e.preventDefault();
    e.stopPropagation();
    post({ t: 'chord', chord });
    return;
  }
  const fns = localShortcuts.get(chord);
  if (fns && fns.length) {
    e.preventDefault();
    for (const fn of fns) {
      try { fn(); } catch (err) { console.error('atrium shortcut handler:', err); }
    }
  }
}, true);

export const atrium = {
  /** Resolves with { window, theme, user } once the shell has answered. */
  ready() { return readyPromise; },
  get context() { return context; },
  get theme() { return context?.theme; },
  on(event, fn) {
    if (!handlers.has(event)) handlers.set(event, []);
    handlers.get(event).push(fn);
  },
  /** App-local shortcuts, active while this window has focus. Chords the
   *  shell has reserved are forwarded there instead and never fire here. */
  shortcuts: {
    on(chord, fn) {
      const c = String(chord);
      if (!localShortcuts.has(c)) localShortcuts.set(c, []);
      localShortcuts.get(c).push(fn);
    },
  },
  setTitle(title) { post({ t: 'set-title', title: String(title) }); },
  close() { post({ t: 'close' }); },
  /** Send `body` to the session; resolves with the session's reply.
   *  `onStream(part)` receives interim parts for streaming kinds. */
  request(body, onStream) {
    seq += 1;
    const req = seq;
    return new Promise((resolve, reject) => {
      pending.set(req, { resolve, reject, onStream });
      post({ t: 'request', req, body });
    });
  },
  /** A live terminal: the user's shell on a pseudo-terminal. Needs
   *  [capabilities] pty in the manifest. Returns handles once open;
   *  `done` resolves when the shell exits. */
  pty({ cols = 80, rows = 24, onData } = {}) {
    seq += 1;
    const req = seq;
    const done = new Promise((resolve, reject) => {
      pending.set(req, { resolve, reject, onStream: (part) => {
        if (onData && part && typeof part.data === 'string') onData(part.data);
      } });
      post({ t: 'request', req, body: { kind: 'pty-open', cols, rows } });
    });
    return {
      write(data) { post({ t: 'request', req: 0, body: { kind: 'pty-input', pty: req, data: String(data) } }); },
      resize(cols, rows) { post({ t: 'request', req: 0, body: { kind: 'pty-resize', pty: req, cols, rows } }); },
      done,
    };
  },
  /** Run a program as the user (argv array — no shell). The app's manifest
   *  must allow argv[0] under [capabilities] exec. `onOutput(text, stream)`
   *  receives output as it happens; resolves with {exit_code, exit_signal,
   *  truncated}. */
  exec(cmd, onOutput) {
    return atrium.request({ kind: 'exec', cmd }, (part) => {
      if (onOutput && part && typeof part.data === 'string') onOutput(part.data, part.stream);
    });
  },
};

// The declarative layer, for apps that are just buttons around commands:
//   <button data-atrium-exec="peipkg list" data-atrium-target="#out">…
// Output streams into the target as text; while running the trigger is
// disabled; the exit status lands as a data attribute on the target.
readyPromise.then(() => {
  document.addEventListener('click', async (e) => {
    const el = e.target.closest('[data-atrium-exec]');
    if (!el || el.disabled) return;
    // v1 split: whitespace, no quoting. A command that needs quoting is a
    // program that should be run through the JS API.
    const cmd = el.dataset.atriumExec.trim().split(/\s+/);
    const target = el.dataset.atriumTarget ? document.querySelector(el.dataset.atriumTarget) : null;
    if (target) { target.textContent = ''; delete target.dataset.exit; }
    el.disabled = true;
    try {
      const done = await atrium.exec(cmd, (text) => { if (target) target.append(text); });
      if (target) target.dataset.exit = done.exit_code ?? `signal ${done.exit_signal}`;
    } catch (err) {
      if (target) { target.textContent = String(err.message || err); target.dataset.exit = 'error'; }
    } finally {
      el.disabled = false;
    }
  });
});

// The handshake. Sent at import time: by the time an app can call anything,
// hello is already on its way.
post({ t: 'hello', v: 1 });
