//! The session's state, and its mirrors.
//!
//! Everything the shell shows that must be the same in every tab lives
//! here: the windows, which one has focus. Each connected shell is a
//! *mirror*: it gets a snapshot on connect and every change after as an
//! event, and it asks for changes by sending requests. It never changes
//! anything itself — a shell that asks to launch an app learns about the
//! resulting window the way every other tab does, from the broadcast.
//!
//! Slice 5: a flat window list and a focus. The tiling tree, per-window
//! state, and the SDK handshake grow from here.

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{apps, ws};

#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub id: u64,
    pub app: String,
    pub title: String,
    pub url: String,
    /// Hidden but alive; its frame stays loaded. Focusing it restores it.
    pub minimized: bool,
}

/// What a shell may ask for.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum Request {
    /// Open `app`. Without `new`, an app that already has a window is
    /// focused rather than opened again — one window per app is the
    /// default; a second is a deliberate ask.
    Launch {
        app: String,
        #[serde(default)]
        new: bool,
    },
    /// Show and focus; a minimised window is restored.
    Focus { id: u64 },
    Minimize { id: u64 },
    Close { id: u64 },
}

/// A connected shell. Its writer is shared between the broadcast path and
/// the connection's own ping replies, behind one mutex, so no two writes
/// ever interleave on the wire — an interleaved frame would corrupt the
/// stream and drop the socket.
struct Mirror {
    id: u64,
    out: Arc<Mutex<UnixStream>>,
}

#[derive(Default)]
pub struct State {
    windows: Vec<Window>,
    focus: Option<u64>,
    next_window: u64,
    next_mirror: u64,
    mirrors: Vec<Mirror>,
}

pub type Shared = Arc<Mutex<State>>;

impl State {
    fn snapshot(&self) -> serde_json::Value {
        json!({ "t": "snapshot", "windows": self.windows, "focus": self.focus })
    }

    /// Send one event to every mirror; a mirror that cannot be written is
    /// gone and is dropped here.
    fn broadcast(&mut self, event: &serde_json::Value) {
        let text = event.to_string();
        self.mirrors.retain(|m| {
            let mut out = m.out.lock().unwrap_or_else(|p| p.into_inner());
            ws::send_text(&mut *out, &text).is_ok()
        });
    }

    /// A shell connected: remember its writer, give it the snapshot.
    pub fn attach(&mut self, out: Arc<Mutex<UnixStream>>) -> u64 {
        self.next_mirror += 1;
        let id = self.next_mirror;
        {
            let mut w = out.lock().unwrap_or_else(|p| p.into_inner());
            let _ = ws::send_text(&mut *w, &self.snapshot().to_string());
        }
        self.mirrors.push(Mirror { id, out });
        id
    }

    pub fn detach(&mut self, mirror: u64) {
        self.mirrors.retain(|m| m.id != mirror);
    }

    pub fn handle(&mut self, req: Request) -> Result<(), String> {
        match req {
            Request::Launch { app, new } => {
                if !new {
                    if let Some(existing) = self.windows.iter().rev().find(|w| w.app == app).map(|w| w.id) {
                        return self.handle(Request::Focus { id: existing });
                    }
                }
                let found = apps::catalogue().into_iter().find(|a| a.id == app).ok_or_else(|| format!("no such app: {app}"))?;
                self.next_window += 1;
                // "About", then "About 2", "About 3" — numbered by how many of
                // this app are open, so two windows are told apart everywhere.
                let n = self.windows.iter().filter(|w| w.app == found.id).count();
                let title = if n == 0 { found.name.clone() } else { format!("{} {}", found.name, n + 1) };
                let w = Window { id: self.next_window, app: found.id, title, url: found.entry, minimized: false };
                self.windows.push(w.clone());
                self.focus = Some(w.id);
                self.broadcast(&json!({ "t": "window.opened", "window": w }));
                self.broadcast(&json!({ "t": "focus", "id": w.id }));
                Ok(())
            }
            Request::Focus { id } => {
                let w = self.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| format!("no such window: {id}"))?;
                if w.minimized {
                    w.minimized = false;
                    self.broadcast(&json!({ "t": "window.restored", "id": id }));
                }
                self.focus = Some(id);
                self.broadcast(&json!({ "t": "focus", "id": id }));
                Ok(())
            }
            Request::Minimize { id } => {
                let w = self.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| format!("no such window: {id}"))?;
                w.minimized = true;
                self.broadcast(&json!({ "t": "window.minimized", "id": id }));
                if self.focus == Some(id) {
                    self.focus = self.windows.iter().rev().find(|w| !w.minimized).map(|w| w.id);
                    self.broadcast(&json!({ "t": "focus", "id": self.focus }));
                }
                Ok(())
            }
            Request::Close { id } => {
                let before = self.windows.len();
                self.windows.retain(|w| w.id != id);
                if self.windows.len() == before {
                    return Err(format!("no such window: {id}"));
                }
                self.broadcast(&json!({ "t": "window.closed", "id": id }));
                if self.focus == Some(id) {
                    // Focus falls to the most recently opened remaining
                    // visible window, or nowhere.
                    self.focus = self.windows.iter().rev().find(|w| !w.minimized).map(|w| w.id);
                    self.broadcast(&json!({ "t": "focus", "id": self.focus }));
                }
                Ok(())
            }
        }
    }
}

/// Run one shell's websocket until it closes.
pub fn serve(state: &Shared, mut conn: UnixStream) {
    // The read half is this thread's alone; the write half is shared with
    // the broadcast path through the mirror's mutex, so replies and
    // broadcasts serialise.
    let Ok(writer) = conn.try_clone().map(|w| Arc::new(Mutex::new(w))) else { return };
    let mirror = state.lock().unwrap_or_else(|p| p.into_inner()).attach(Arc::clone(&writer));
    while let Some(frame) = ws::read_frame(&mut conn) {
        match frame.opcode {
            ws::TEXT => {
                let reply = match serde_json::from_slice::<Request>(&frame.payload) {
                    Ok(req) => state.lock().unwrap_or_else(|p| p.into_inner()).handle(req).err(),
                    Err(e) => Some(format!("bad request: {e}")),
                };
                if let Some(message) = reply {
                    // Errors go only to the asker; they are not state.
                    let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
                    let _ = ws::send_text(&mut *w, &json!({ "t": "error", "message": message }).to_string());
                }
            }
            ws::PING => {
                let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
                let _ = ws::write_frame(&mut *w, ws::PONG, &frame.payload);
            }
            ws::CLOSE => {
                let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
                let _ = ws::write_frame(&mut *w, ws::CLOSE, &[]);
                break;
            }
            _ => {}
        }
    }
    state.lock().unwrap_or_else(|p| p.into_inner()).detach(mirror);
}
