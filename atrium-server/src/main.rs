//! atrium-server — the network-facing half of Atrium.
//!
//! Spawned by atriumd with a privilege-stripped token and one socketpair on
//! fd 3. It owns the listener, serves the login page to browsers without a
//! session, drives the logon conversation through atriumd for those logging
//! in, binds a cookie to the session that results, and from then on forwards
//! that browser's connections to the session's host.
//!
//! What it holds is exactly the routing view: cookie -> session -> the
//! session host's control socket. No token, no privilege, no idea what a
//! session's process is. That is the property the split exists for; nothing
//! in here should ever need more.
//!
//! Forwarding is per connection: a fresh socketpair, one end sent to the
//! host with a `SessionMessage::Connection`, the bytes pumped through the
//! other. The host sees ordinary HTTP with the cookie stripped and two
//! headers added saying which browser it is talking to. That is trustworthy
//! only because nothing but this process can reach a host — keep it so.

mod log;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atrium_http::{Request as HttpRequest, Response, read_request, write_response};
use atrium_proto::{Answer, CONTROL_FD, Reply, Request, SessionMessage, read_frame_fd, write_frame, write_frame_fd};
use serde::Deserialize;
use serde_json::json;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const COOKIE: &str = "atrium";

const LOGIN_PAGE: &str = include_str!("login.html");

/// A session host the server can reach.
struct SessionLink {
    id: u64,
    username: String,
    /// Our end of the host's control socket. One frame per connection.
    control: Mutex<UnixStream>,
}

/// One browser's binding to a session.
struct BrowserSession {
    /// A random tag distinct from the bearer, for the host to tell browsers
    /// apart. Never the cookie itself.
    id: String,
    session: u64,
}

struct State {
    /// The control socket to atriumd. Request/reply in lockstep, so one lock
    /// covers a whole exchange.
    control: Mutex<UnixStream>,
    cookies: Mutex<HashMap<String, BrowserSession>>,
    sessions: Mutex<HashMap<u64, Arc<SessionLink>>>,
    next_conv: AtomicU64,
}

impl State {
    fn call(&self, request: &Request) -> std::io::Result<(Reply, Option<OwnedFd>)> {
        let c = self.control.lock().unwrap_or_else(|p| p.into_inner());
        write_frame(&mut &*c, request)?;
        read_frame_fd(&c)
    }

    /// Forget a session and every cookie bound to it.
    fn drop_session(&self, id: u64) {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        self.cookies.lock().unwrap_or_else(|p| p.into_inner()).retain(|_, b| b.session != id);
    }
}

/// 256 bits from the kernel, hex.
fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn set_cookie(value: &str) -> String {
    // Secure is added when TLS exists; until then the cookie is as exposed as
    // the password that produced it.
    format!("Set-Cookie: {COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict")
}

fn clear_cookie() -> String {
    format!("Set-Cookie: {COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

#[derive(Deserialize)]
struct StartBody {
    username: String,
}

#[derive(Deserialize)]
struct AnswerBody {
    conv: u64,
    answers: Vec<Answer>,
}

fn reply_json(reply: Reply) -> Response {
    let v = serde_json::to_value(&reply).unwrap_or_else(|_| json!({"type":"error","reason":"unencodable reply"}));
    Response::json(&v)
}

/// What to do with a request: answer it here, or hand it to a session host.
enum Action {
    Respond(Response),
    Forward { link: Arc<SessionLink>, browser_session: String },
}

fn handle(state: &State, req: &HttpRequest, peer: &str) -> Action {
    let bound = req.cookie(COOKIE).and_then(|c| {
        let cookies = state.cookies.lock().unwrap_or_else(|p| p.into_inner());
        let b = cookies.get(c)?;
        let link = state.sessions.lock().unwrap_or_else(|p| p.into_inner()).get(&b.session).cloned()?;
        Some((b.id.clone(), link))
    });

    // Logged in: the server keeps logout for itself and forwards the rest.
    if let Some((browser_session, link)) = bound {
        if req.method == "POST" && req.path == "/api/logout" {
            let cookie = req.cookie(COOKIE).unwrap_or_default().to_string();
            state.cookies.lock().unwrap_or_else(|p| p.into_inner()).remove(&cookie);
            // Slice 3: one cookie per session, so logging the browser out
            // ends the session. With the picker this becomes "end it when
            // no cookie remains".
            state.drop_session(link.id);
            if let Err(e) = state.call(&Request::Logout { session: link.id }) {
                log::error(format_args!("atriumd: {e}"));
            }
            log::info(format_args!("{} logged out (session {})", link.username, link.id));
            return Action::Respond(see_other("/").with_header(clear_cookie()));
        }
        return Action::Forward { link, browser_session };
    }

    Action::Respond(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => Response::html(LOGIN_PAGE),

        ("POST", "/api/logon/start") => {
            let Ok(body) = serde_json::from_slice::<StartBody>(&req.body) else {
                return Action::Respond(Response::status(400, "Bad Request"));
            };
            let username = body.username.trim().to_string();
            if username.is_empty() || username.len() > 256 {
                return Action::Respond(Response::status(400, "Bad Request"));
            }
            let conv = state.next_conv.fetch_add(1, Ordering::Relaxed);
            match state.call(&Request::LogonStart { conv, username, remote: peer.to_string() }) {
                Ok((reply, fd)) => finish(state, reply, fd),
                Err(e) => {
                    log::error(format_args!("atriumd: {e}"));
                    Response::status(502, "Session host unavailable")
                }
            }
        }

        ("POST", "/api/logon/answer") => {
            let Ok(body) = serde_json::from_slice::<AnswerBody>(&req.body) else {
                return Action::Respond(Response::status(400, "Bad Request"));
            };
            match state.call(&Request::LogonAnswer { conv: body.conv, answers: body.answers }) {
                Ok((reply, fd)) => finish(state, reply, fd),
                Err(e) => {
                    log::error(format_args!("atriumd: {e}"));
                    Response::status(502, "Session host unavailable")
                }
            }
        }

        // A stale cookie (session gone) lands here for anything else:
        // clear it and send the browser back to the login page.
        (_, "/api/logout") => see_other("/").with_header(clear_cookie()),
        _ if req.cookie(COOKIE).is_some() => see_other("/").with_header(clear_cookie()),
        (_, p) if p.starts_with("/api/") => Response::status(401, "Unauthorized"),
        _ => Response::status(404, "Not Found"),
    })
}

fn see_other(location: &str) -> Response {
    Response { status: 303, reason: "See Other", content_type: "text/plain", extra_headers: vec![format!("Location: {location}")], body: vec![] }
}

/// Turn a logon reply into the HTTP answer, binding a cookie on a grant.
fn finish(state: &State, reply: Reply, fd: Option<OwnedFd>) -> Response {
    if let Reply::Granted { session, ref username, .. } = reply {
        let Some(fd) = fd else {
            log::error(format_args!("grant for {username} carried no session socket"));
            return Response::status(500, "Internal Server Error");
        };
        let (cookie, tag) = match (random_hex(32), random_hex(8)) {
            (Ok(c), Ok(t)) => (c, t),
            _ => {
                log::error(format_args!("no randomness for a cookie"));
                return Response::status(500, "Internal Server Error");
            }
        };
        let link = Arc::new(SessionLink { id: session, username: username.clone(), control: Mutex::new(UnixStream::from(fd)) });
        state.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(session, link);
        state.cookies.lock().unwrap_or_else(|p| p.into_inner()).insert(cookie.clone(), BrowserSession { id: tag, session });
        log::info(format_args!("{username} logged in (session {session})"));
        return reply_json(reply).with_header(set_cookie(&cookie));
    }
    reply_json(reply)
}

/// Hand one connection to a session host and pump bytes until either side
/// is done. Returns false if the host could not be reached (it is gone).
fn forward(link: &SessionLink, browser_session: &str, peer: &str, req: &HttpRequest, tcp: &mut TcpStream) -> bool {
    let (ours, theirs) = match UnixStream::pair() {
        Ok(p) => p,
        Err(e) => {
            log::error(format_args!("socketpair: {e}"));
            return true;
        }
    };
    {
        let control = link.control.lock().unwrap_or_else(|p| p.into_inner());
        let msg = SessionMessage::Connection { browser_session: browser_session.to_string(), client_addr: peer.to_string() };
        if write_frame_fd(&control, &msg, Some(theirs.as_fd())).is_err() {
            return false;
        }
    }
    drop(theirs);

    // Replay what we already read, rewritten: the bearer stays here.
    let head = req.rewritten_head(
        &["cookie", "x-atrium-browser-session", "x-atrium-client-addr"],
        &[format!("X-Atrium-Browser-Session: {browser_session}"), format!("X-Atrium-Client-Addr: {peer}")],
    );
    let mut to_host = ours;
    if to_host.write_all(&head).is_err() || to_host.write_all(&req.body).is_err() {
        return true;
    }

    // Then plain bytes both ways, for whatever the connection turns into.
    let _ = tcp.set_read_timeout(None);
    let mut from_host = match to_host.try_clone() {
        Ok(s) => s,
        Err(_) => return true,
    };
    let mut tcp_in = match tcp.try_clone() {
        Ok(s) => s,
        Err(_) => return true,
    };
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut tcp_in, &mut to_host);
        let _ = to_host.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut from_host, tcp);
    let _ = tcp.shutdown(Shutdown::Both);
    let _ = up.join();
    true
}

fn serve(state: Arc<State>, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let Some(req) = read_request(&mut stream) else {
        write_response(&mut stream, &Response::status(400, "Bad Request"));
        return;
    };
    match handle(&state, &req, &peer) {
        Action::Respond(r) => write_response(&mut stream, &r),
        Action::Forward { link, browser_session } => {
            if !forward(&link, &browser_session, &peer, &req, &mut stream) {
                // The host is gone. Everything bound to it goes too, and
                // this browser starts over.
                log::warn(format_args!("session {} ({}) is unreachable; dropping it", link.id, link.username));
                state.drop_session(link.id);
                write_response(&mut stream, &see_other("/").with_header(clear_cookie()));
            }
        }
    }
}

fn main() -> std::process::ExitCode {
    // SAFETY: atriumd placed the control socket on CONTROL_FD before exec and
    // nothing else in this process refers to it.
    let control = unsafe { UnixStream::from_raw_fd(CONTROL_FD) };
    let listener = match TcpListener::bind(LISTEN_ADDR) {
        Ok(l) => l,
        Err(e) => {
            log::error(format_args!("listen on {LISTEN_ADDR}: {e}"));
            return std::process::ExitCode::FAILURE;
        }
    };
    log::info(format_args!("listening on http://{LISTEN_ADDR}"));
    let state = Arc::new(State {
        control: Mutex::new(control),
        cookies: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
        next_conv: AtomicU64::new(1),
    });
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || serve(state, stream));
            }
            Err(e) => log::warn(format_args!("accept: {e}")),
        }
    }
    std::process::ExitCode::SUCCESS
}
