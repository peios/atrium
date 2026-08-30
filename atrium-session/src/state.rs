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

use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
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
    /// An app renamed its own window (via the SDK, relayed by the shell).
    SetTitle { id: u64, title: String },
    /// An app's request, relayed by the shell, which vouches for `win`.
    /// Answered to the asking mirror only, correlated by `req`.
    App { win: u64, req: u64, body: serde_json::Value },
}

/// A connected shell. Its writer is shared between the broadcast path and
/// the connection's own ping replies, behind one mutex, so no two writes
/// ever interleave on the wire — an interleaved frame would corrupt the
/// stream and drop the socket.
struct Mirror {
    id: u64,
    out: Arc<Mutex<UnixStream>>,
}

/// The session's chord table: what the shell (and, through the SDK, every
/// app frame) treats as reserved, and what each chord does. Data, not
/// code: it ships in the snapshot, so rebinding later is a request and a
/// broadcast, nothing structural. Actions are namespaced — today all are
/// shell actions ("close-window"); a future app-owned global would be
/// "app:<id>:<name>", which is why the action is a string and not an enum
/// on the wire.
fn default_keymap() -> Vec<serde_json::Value> {
    let mut map = vec![
        json!({ "chord": "Ctrl+K", "action": "command-bar" }),
        json!({ "chord": "Alt+T", "action": "toolbox" }),
        json!({ "chord": "Alt+W", "action": "close-window" }),
        json!({ "chord": "Alt+M", "action": "minimize-window" }),
        json!({ "chord": "Ctrl+`", "action": "cycle-window" }),
    ];
    for n in 1..=9 {
        map.push(json!({ "chord": format!("Alt+{n}"), "action": format!("focus-window-{n}") }));
    }
    map
}

/// A live pseudo-terminal: the master's write half and the shell's pid.
/// Keyed by the opening request's id (per connection, but ids are minted
/// per frame by the SDK and scoped here with the window id).
pub struct Pty {
    master: OwnedFd,
    pid: libc::pid_t,
    win: u64,
}

pub type PtyMap = Arc<Mutex<HashMap<(u64, u64), Pty>>>;

#[derive(Default)]
pub struct State {
    pub ptys: PtyMap,
    windows: Vec<Window>,
    focus: Option<u64>,
    next_window: u64,
    next_mirror: u64,
    mirrors: Vec<Mirror>,
}

pub type Shared = Arc<Mutex<State>>;

impl State {
    fn snapshot(&self) -> serde_json::Value {
        json!({ "t": "snapshot", "windows": self.windows, "focus": self.focus, "keymap": default_keymap() })
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

    pub fn app_of_window(&self, win: u64) -> Option<String> {
        self.windows.iter().find(|w| w.id == win).map(|w| w.app.clone())
    }

    pub fn detach(&mut self, mirror: u64) {
        self.mirrors.retain(|m| m.id != mirror);
    }

    /// Handle one request. `Ok(Some(v))` is a direct reply for the asking
    /// mirror alone; state changes travel by broadcast as always.
    pub fn handle(&mut self, req: Request) -> Result<Option<serde_json::Value>, String> {
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
                Ok(None)
            }
            Request::Focus { id } => {
                let w = self.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| format!("no such window: {id}"))?;
                if w.minimized {
                    w.minimized = false;
                    self.broadcast(&json!({ "t": "window.restored", "id": id }));
                }
                self.focus = Some(id);
                self.broadcast(&json!({ "t": "focus", "id": id }));
                Ok(None)
            }
            Request::Minimize { id } => {
                let w = self.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| format!("no such window: {id}"))?;
                w.minimized = true;
                self.broadcast(&json!({ "t": "window.minimized", "id": id }));
                if self.focus == Some(id) {
                    self.focus = self.windows.iter().rev().find(|w| !w.minimized).map(|w| w.id);
                    self.broadcast(&json!({ "t": "focus", "id": self.focus }));
                }
                Ok(None)
            }
            Request::SetTitle { id, title } => {
                let title: String = title.chars().take(120).collect();
                if title.trim().is_empty() {
                    return Err("empty title".into());
                }
                let w = self.windows.iter_mut().find(|w| w.id == id).ok_or_else(|| format!("no such window: {id}"))?;
                w.title = title.clone();
                self.broadcast(&json!({ "t": "window.titled", "id": id, "title": title }));
                Ok(None)
            }
            Request::App { win, req, body } => {
                let Some(window) = self.windows.iter().find(|w| w.id == win) else {
                    return Err(format!("no such window: {win}"));
                };
                let reply = app_request(&window.app, &body);
                Ok(Some(json!({ "t": "app.reply", "win": win, "req": req, "body": reply })))
            }
            Request::Close { id } => {
                let before = self.windows.len();
                self.windows.retain(|w| w.id != id);
                if self.windows.len() == before {
                    return Err(format!("no such window: {id}"));
                }
                // A window's terminals die with it; the reader thread sees
                // EOF and sends the final reply.
                let mut ptys = self.ptys.lock().unwrap_or_else(|p| p.into_inner());
                ptys.retain(|_, p| {
                    if p.win == id {
                        // SAFETY: signalling the shell's process group.
                        unsafe { libc::kill(-p.pid, libc::SIGHUP) };
                        false
                    } else {
                        true
                    }
                });
                drop(ptys);
                self.broadcast(&json!({ "t": "window.closed", "id": id }));
                if self.focus == Some(id) {
                    // Focus falls to the most recently opened remaining
                    // visible window, or nowhere.
                    self.focus = self.windows.iter().rev().find(|w| !w.minimized).map(|w| w.id);
                    self.broadcast(&json!({ "t": "focus", "id": self.focus }));
                }
                Ok(None)
            }
        }
    }
}

/// Answer one app request. The session is the user, so everything here
/// runs as them; `app` is which app's window asked — the unit capability
/// checks apply to.
fn app_request(app: &str, body: &serde_json::Value) -> serde_json::Value {
    let kind = body.get("kind").and_then(serde_json::Value::as_str).unwrap_or("");
    let _ = app;
    match kind {
        "system-info" => {
            let read = |p: &str| std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default();
            let uptime = std::fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|s| s.split_whitespace().next().and_then(|f| f.parse::<f64>().ok()))
                .unwrap_or(0.0) as u64;
            json!({
                "os": "Peios",
                "atrium_version": env!("CARGO_PKG_VERSION"),
                "hostname": read("/proc/sys/kernel/hostname"),
                "kernel": read("/proc/sys/kernel/osrelease"),
                "uptime_seconds": uptime,
                "user": std::env::var("USER").unwrap_or_default(),
                "display_name": std::env::var("ATRIUM_DISPLAY_NAME").unwrap_or_default(),
                "logon_session": std::env::var("ATRIUM_SESSION").unwrap_or_default(),
                "session_pid": std::process::id(),
            })
        }
        other => json!({ "error": format!("unknown request kind: {other}") }),
    }
}

/// Cap on one exec's streamed output; past it the process is killed.
const EXEC_OUTPUT_CAP: usize = 1024 * 1024;
/// Cap on one exec's runtime.
const EXEC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Run one `exec` request: spawn as the user (the session *is* the user),
/// stream output to the asking mirror as `app.stream` frames, finish with
/// the ordinary `app.reply`. The allowlist is the app's manifest: argv[0]
/// must be listed (or the list holds "*"). No shell is involved — argv is
/// exec'd as given.
fn run_exec(win: u64, req: u64, app: String, body: serde_json::Value, writer: Arc<Mutex<UnixStream>>) {
    let send = move |v: serde_json::Value| {
        let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
        let _ = ws::send_text(&mut *w, &v.to_string());
    };
    let fail = move |send: &dyn Fn(serde_json::Value), reason: String| {
        send(json!({ "t": "app.reply", "win": win, "req": req, "body": { "error": reason } }));
    };
    let argv: Vec<String> = match body.get("cmd").and_then(serde_json::Value::as_array) {
        Some(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        None => return fail(&send, "exec needs a cmd array".into()),
    };
    if argv.is_empty() || argv.iter().any(|a| a.contains(' ')) {
        return fail(&send, "exec needs a non-empty cmd".into());
    }
    let allow = apps::capabilities(&app).exec;
    let program = argv[0].clone();
    if !allow.iter().any(|a| a == "*" || *a == program) {
        return fail(&send, format!("{app} may not exec {program}: not in its manifest's capabilities.exec"));
    }
    std::thread::spawn(move || {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let started = std::time::Instant::now();
        let mut child = match Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return fail(&send, format!("could not run {}: {e}", argv[0])),
        };
        // stderr on its own thread; both streams feed the same sender.
        let err_send = send.clone();
        let mut stderr = child.stderr.take().expect("piped");
        let err_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut total = 0usize;
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 || total > EXEC_OUTPUT_CAP {
                    break;
                }
                total += n;
                err_send(json!({ "t": "app.stream", "win": win, "req": req,
                                 "body": { "stream": "stderr", "data": String::from_utf8_lossy(&buf[..n]) } }));
            }
        });
        let mut stdout = child.stdout.take().expect("piped");
        let mut buf = [0u8; 8192];
        let mut total = 0usize;
        let mut truncated = false;
        loop {
            if started.elapsed() > EXEC_TIMEOUT || total > EXEC_OUTPUT_CAP {
                truncated = true;
                let _ = child.kill();
                break;
            }
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n;
                    send(json!({ "t": "app.stream", "win": win, "req": req,
                                 "body": { "stream": "stdout", "data": String::from_utf8_lossy(&buf[..n]) } }));
                }
            }
        }
        let status = child.wait();
        let _ = err_thread.join();
        let (code, signal) = match &status {
            Ok(s) => {
                use std::os::unix::process::ExitStatusExt;
                (s.code(), s.signal())
            }
            Err(_) => (None, None),
        };
        send(json!({ "t": "app.reply", "win": win, "req": req,
                     "body": { "exit_code": code, "exit_signal": signal, "truncated": truncated } }));
    });
}

/// Open a pseudo-terminal running the user's shell for window `win`,
/// stream its output as `app.stream` frames, and answer the opening
/// request only when the shell exits. Input and resizes arrive as later
/// `pty-input` / `pty-resize` requests naming the opening request's id.
fn run_pty(win: u64, req: u64, app: String, body: serde_json::Value, writer: Arc<Mutex<UnixStream>>, ptys: PtyMap) {
    let send = move |v: serde_json::Value| {
        let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
        let _ = ws::send_text(&mut *w, &v.to_string());
    };
    let fail = move |send: &dyn Fn(serde_json::Value), reason: String| {
        send(json!({ "t": "app.reply", "win": win, "req": req, "body": { "error": reason } }));
    };
    if !apps::capabilities(&app).pty {
        return fail(&send, format!("{app} may not open a terminal: its manifest does not declare capabilities.pty"));
    }
    let cols = body.get("cols").and_then(serde_json::Value::as_u64).unwrap_or(80) as u16;
    let rows = body.get("rows").and_then(serde_json::Value::as_u64).unwrap_or(24) as u16;
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let mut ws_size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills the two fds; the winsize is ours.
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws_size) } != 0 {
        return fail(&send, format!("openpty: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: fresh fds from openpty.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    let shell = std::env::var("SHELL").ok().filter(|s| s.starts_with('/')).unwrap_or_else(|| "/bin/sh".into());
    let Ok(shell_c) = std::ffi::CString::new(shell.clone()) else {
        return fail(&send, "bad shell path".into());
    };
    // The session has threads; between fork and exec only async-signal-safe
    // calls happen.
    // SAFETY: fork + setsid/ioctl/dup2/exec in the child, on fds we own.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return fail(&send, format!("fork: {}", std::io::Error::last_os_error()));
    }
    if pid == 0 {
        // SAFETY: child; nothing below returns.
        unsafe {
            libc::setsid();
            libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY, 0);
            libc::dup2(slave.as_raw_fd(), 0);
            libc::dup2(slave.as_raw_fd(), 1);
            libc::dup2(slave.as_raw_fd(), 2);
            let term = c"TERM=xterm-256color";
            libc::putenv(term.as_ptr() as *mut libc::c_char);
            let argv = [shell_c.as_ptr(), std::ptr::null()];
            libc::execv(shell_c.as_ptr(), argv.as_ptr());
            libc::_exit(127);
        }
    }
    drop(slave);
    let Ok(reader) = master.try_clone() else {
        return fail(&send, "could not clone the pty".into());
    };
    ptys.lock().unwrap_or_else(|p| p.into_inner()).insert((win, req), Pty { master, pid, win });
    std::thread::spawn(move || {
        use std::io::Read;
        let mut f = std::fs::File::from(reader);
        let mut buf = [0u8; 8192];
        loop {
            match f.read(&mut buf) {
                // EIO is the normal end of a pty: the shell exited.
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Terminal bytes may be split mid-UTF-8; lossy is fine for
                    // v1 and the terminal redraws constantly.
                    send(json!({ "t": "app.stream", "win": win, "req": req,
                                 "body": { "data": String::from_utf8_lossy(&buf[..n]) } }));
                }
            }
        }
        let mut status = 0;
        // SAFETY: waitpid on our own child.
        unsafe { libc::waitpid(pid, &mut status, 0) };
        ptys.lock().unwrap_or_else(|p| p.into_inner()).remove(&(win, req));
        let code = if libc::WIFEXITED(status) { Some(libc::WEXITSTATUS(status)) } else { None };
        send(json!({ "t": "app.reply", "win": win, "req": req, "body": { "exit_code": code } }));
    });
}

/// Write input or a resize to a window's open pty. Fire-and-forget: the
/// SDK sends these with req 0 and expects no reply.
fn pty_message(kind: &str, win: u64, body: &serde_json::Value, ptys: &PtyMap) {
    let Some(target) = body.get("pty").and_then(serde_json::Value::as_u64) else { return };
    let ptys = ptys.lock().unwrap_or_else(|p| p.into_inner());
    let Some(pty) = ptys.get(&(win, target)) else { return };
    match kind {
        "pty-input" => {
            if let Some(data) = body.get("data").and_then(serde_json::Value::as_str) {
                use std::io::Write;
                let mut f = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(pty.master.as_raw_fd()) });
                let _ = f.write_all(data.as_bytes());
            }
        }
        "pty-resize" => {
            let cols = body.get("cols").and_then(serde_json::Value::as_u64).unwrap_or(80) as u16;
            let rows = body.get("rows").and_then(serde_json::Value::as_u64).unwrap_or(24) as u16;
            let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
            // SAFETY: ioctl on a pty master we own.
            unsafe { libc::ioctl(pty.master.as_raw_fd(), libc::TIOCSWINSZ, &size) };
        }
        _ => {}
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
                let outcome = match serde_json::from_slice::<Request>(&frame.payload) {
                    // exec and pty stream; they get the asker's writer and
                    // answer on their own schedule. pty-input/resize are
                    // fire-and-forget. Everything else is synchronous.
                    Ok(Request::App { win, req, body })
                        if matches!(
                            body.get("kind").and_then(serde_json::Value::as_str),
                            Some("exec" | "pty-open" | "pty-input" | "pty-resize")
                        ) =>
                    {
                        let kind = body.get("kind").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                        let (app, ptys) = {
                            let st = state.lock().unwrap_or_else(|p| p.into_inner());
                            (st.app_of_window(win), Arc::clone(&st.ptys))
                        };
                        match app {
                            Some(app) => {
                                match kind.as_str() {
                                    "exec" => run_exec(win, req, app, body, Arc::clone(&writer)),
                                    "pty-open" => run_pty(win, req, app, body, Arc::clone(&writer), ptys),
                                    other => pty_message(other, win, &body, &ptys),
                                }
                                Ok(None)
                            }
                            None => Err(format!("no such window: {win}")),
                        }
                    }
                    Ok(req) => state.lock().unwrap_or_else(|p| p.into_inner()).handle(req),
                    Err(e) => Err(format!("bad request: {e}")),
                };
                // Direct replies and errors go only to the asker; they are
                // not state.
                let direct = match outcome {
                    Ok(Some(v)) => Some(v),
                    Ok(None) => None,
                    Err(message) => Some(json!({ "t": "error", "message": message })),
                };
                if let Some(v) = direct {
                    let mut w = writer.lock().unwrap_or_else(|p| p.into_inner());
                    let _ = ws::send_text(&mut *w, &v.to_string());
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
