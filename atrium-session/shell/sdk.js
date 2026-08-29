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
      document.documentElement.dataset.theme = m.theme;
      announceReady(context);
      break;
    case 'theme':
      if (context) context.theme = m.theme;
      document.documentElement.dataset.theme = m.theme;
      emit('theme', m.theme);
      break;
    case 'reply': {
      const p = pending.get(m.req);
      if (!p) return;
      pending.delete(m.req);
      if (m.error) p.reject(new Error(m.error));
      else p.resolve(m.body);
      break;
    }
  }
});

function post(msg) {
  window.parent.postMessage(msg, '*');
}

export const atrium = {
  /** Resolves with { window, theme, user } once the shell has answered. */
  ready() { return readyPromise; },
  get context() { return context; },
  get theme() { return context?.theme; },
  on(event, fn) {
    if (!handlers.has(event)) handlers.set(event, []);
    handlers.get(event).push(fn);
  },
  setTitle(title) { post({ t: 'set-title', title: String(title) }); },
  close() { post({ t: 'close' }); },
  /** Send `body` to the session; resolves with the session's reply. */
  request(body) {
    seq += 1;
    const req = seq;
    return new Promise((resolve, reject) => {
      pending.set(req, { resolve, reject });
      post({ t: 'request', req, body });
    });
  },
};

// The handshake. Sent at import time: by the time an app can call anything,
// hello is already on its way.
post({ t: 'hello', v: 1 });
