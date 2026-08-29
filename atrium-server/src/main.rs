//! atrium-server — the network-facing half of Atrium.
//!
//! Spawned by atriumd with a privilege-stripped token and one socketpair on
//! fd 3. It owns the listener, serves the login page to browsers without a
//! session, drives the logon conversation through atriumd for those logging
//! in, and binds a cookie to the session that results.
//!
//! What it holds is exactly the routing view: cookie -> session. No token,
//! no privilege, no idea what a session's process is. That is the property
//! the split exists for; nothing in here should ever need more.
//!
//! Slice 2: after login the page is a placeholder served here, because there
//! is no session host yet to forward to. Forwarding arrives with the host.

mod http;
mod log;

use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use atrium_proto::{Answer, CONTROL_FD, Reply, Request, read_frame, write_frame};
use http::{Request as HttpRequest, Response};
use serde::Deserialize;
use serde_json::json;

const LISTEN_ADDR: &str = "0.0.0.0:8080";
const COOKIE: &str = "atrium";

const LOGIN_PAGE: &str = include_str!("login.html");

/// One browser's binding to a session.
struct BrowserSession {
    session: u64,
    username: String,
    display_name: String,
}

struct State {
    /// The control socket to atriumd. Request/reply in lockstep, so one lock
    /// covers a whole exchange.
    control: Mutex<UnixStream>,
    cookies: Mutex<HashMap<String, BrowserSession>>,
    next_conv: AtomicU64,
}

impl State {
    fn call(&self, request: &Request) -> std::io::Result<Reply> {
        let mut c = self.control.lock().unwrap_or_else(|p| p.into_inner());
        write_frame(&mut *c, request)?;
        read_frame(&mut *c)
    }
}

/// 256 bits from the kernel, hex. The bearer; it goes into the cookie jar
/// and nowhere else.
fn new_cookie() -> std::io::Result<String> {
    let mut b = [0u8; 32];
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

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn session_page(b: &BrowserSession) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Atrium</title>\
<style>:root{{color-scheme:light dark;font-family:system-ui,sans-serif}}body{{margin:2rem}}button{{font:inherit}}</style></head>\
<body><h1>Hello {}</h1><p>Logged in as <code>{}</code>, logon session <code>{}</code>.</p>\
<p>This is atrium-server's placeholder: the session host that will own this page is spawned through peinit's jobs API, which is not there yet.</p>\
<form method=\"post\" action=\"/api/logout\"><button>Log out</button></form></body></html>",
        escape(&b.display_name),
        escape(&b.username),
        b.session
    )
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

fn handle(state: &State, req: &HttpRequest, peer: String) -> Response {
    let bound = req.cookie(COOKIE).and_then(|c| {
        let cookies = state.cookies.lock().unwrap_or_else(|p| p.into_inner());
        cookies.get(c).map(|b| BrowserSession { session: b.session, username: b.username.clone(), display_name: b.display_name.clone() })
    });

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => match bound {
            Some(b) => Response::html(&session_page(&b)),
            None => Response::html(LOGIN_PAGE),
        },

        ("POST", "/api/logon/start") => {
            if bound.is_some() {
                return Response::status(409, "Already logged in");
            }
            let Ok(body) = serde_json::from_slice::<StartBody>(&req.body) else {
                return Response::status(400, "Bad Request");
            };
            let username = body.username.trim().to_string();
            if username.is_empty() || username.len() > 256 {
                return Response::status(400, "Bad Request");
            }
            let conv = state.next_conv.fetch_add(1, Ordering::Relaxed);
            match state.call(&Request::LogonStart { conv, username, remote: peer }) {
                Ok(reply) => finish(state, reply),
                Err(e) => {
                    log::error(format_args!("atriumd: {e}"));
                    Response::status(502, "Session host unavailable")
                }
            }
        }

        ("POST", "/api/logon/answer") => {
            let Ok(body) = serde_json::from_slice::<AnswerBody>(&req.body) else {
                return Response::status(400, "Bad Request");
            };
            match state.call(&Request::LogonAnswer { conv: body.conv, answers: body.answers }) {
                Ok(reply) => finish(state, reply),
                Err(e) => {
                    log::error(format_args!("atriumd: {e}"));
                    Response::status(502, "Session host unavailable")
                }
            }
        }

        ("POST", "/api/logout") => {
            if let Some(c) = req.cookie(COOKIE) {
                let removed = state.cookies.lock().unwrap_or_else(|p| p.into_inner()).remove(c);
                if let Some(b) = removed {
                    // Slice 2: one cookie per session, so logging the browser
                    // out ends the session. With the picker, this becomes
                    // "drop this cookie; end the session when none remain".
                    if let Err(e) = state.call(&Request::Logout { session: b.session }) {
                        log::error(format_args!("atriumd: {e}"));
                    }
                }
            }
            Response { status: 303, reason: "See Other", content_type: "text/plain", extra_headers: vec!["Location: /".into(), clear_cookie()], body: vec![] }
        }

        ("GET", "/api/whoami") => match bound {
            Some(b) => Response::json(&json!({"username": b.username, "display_name": b.display_name, "session": b.session})),
            None => Response::status(401, "Unauthorized"),
        },

        _ => Response::status(404, "Not Found"),
    }
}

/// Turn a logon reply into the HTTP answer, binding a cookie on a grant.
fn finish(state: &State, reply: Reply) -> Response {
    if let Reply::Granted { session, ref username, ref display_name, .. } = reply {
        let cookie = match new_cookie() {
            Ok(c) => c,
            Err(e) => {
                log::error(format_args!("no randomness for a cookie: {e}"));
                return Response::status(500, "Internal Server Error");
            }
        };
        state.cookies.lock().unwrap_or_else(|p| p.into_inner()).insert(
            cookie.clone(),
            BrowserSession { session, username: username.clone(), display_name: display_name.clone() },
        );
        log::info(format_args!("{username} logged in (session {session})"));
        return reply_json(reply).with_header(set_cookie(&cookie));
    }
    reply_json(reply)
}

fn serve(state: Arc<State>, mut stream: TcpStream) {
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let Some(req) = http::read_request(&mut stream) else {
        http::write_response(&mut stream, &Response::status(400, "Bad Request"));
        return;
    };
    let response = handle(&state, &req, peer);
    http::write_response(&mut stream, &response);
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
    let state = Arc::new(State { control: Mutex::new(control), cookies: Mutex::new(HashMap::new()), next_conv: AtomicU64::new(1) });
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
