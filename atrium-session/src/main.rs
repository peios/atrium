//! atrium-session — one interactive session, as the user.
//!
//! Started with the user's primary token installed and one socketpair on fd
//! 3 whose other end atrium-server holds. It has no privilege and needs
//! none: it is the user. Every browser connection bound to this session
//! arrives as a `SessionMessage::Connection` frame carrying a descriptor,
//! already authenticated, already tagged with which browser it came from.
//! "Accept" here means "receive a descriptor".
//!
//! It serves the shell — the chrome the user lives in — from files built
//! into the binary, answers the shell's API, and holds the session's state
//! (`state`), which every connected shell mirrors over a websocket.

mod apps;
mod state;
mod ws;

use std::io::Write;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;

use atrium_http::{Response, read_request, write_response};
use atrium_proto::{CONTROL_FD, SessionMessage, read_frame_fd};
use peios::token::{Token, TokenAccess};
use serde_json::json;

const SHELL_HTML: &str = include_str!("../shell/index.html");
const SHELL_CSS: &str = include_str!("../shell/shell.css");
const SHELL_JS: &str = include_str!("../shell/shell.js");
const SHELL_ICONS: &str = include_str!("../shell/icons.js");
const SHELL_SDK: &str = include_str!("../shell/sdk.js");

fn asset(body: &'static str, content_type: &'static str) -> Response {
    Response { status: 200, reason: "OK", content_type, extra_headers: vec![], body: body.as_bytes().to_vec() }
}

struct Identity {
    user_sid: String,
    logon_session: String,
}

fn identity() -> Identity {
    let tok = Token::open_self(true, TokenAccess::QUERY);
    let user_sid = tok.as_ref().ok().and_then(|t| t.user().ok()).map(|s| s.to_string()).unwrap_or_else(|| "unknown".into());
    let logon_session = tok.as_ref().ok().and_then(|t| t.auth_id().ok()).map(|s| format!("{s:?}")).unwrap_or_else(|| "unknown".into());
    Identity { user_sid, logon_session }
}

fn serve(mut conn: UnixStream, id: &Identity, state: &state::Shared) {
    let Some(req) = read_request(&mut conn) else {
        write_response(&mut conn, &Response::status(400, "Bad Request"));
        return;
    };
    if req.method == "GET" && req.path == "/ws" {
        if !ws::is_upgrade(&req) {
            write_response(&mut conn, &Response::status(426, "Upgrade Required"));
            return;
        }
        if ws::accept(&mut conn, &req).is_ok() {
            state::serve(state, conn);
        }
        return;
    }
    let resp = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/whoami") => Response::json(&json!({
            "user": std::env::var("USER").unwrap_or_default(),
            "display_name": std::env::var("ATRIUM_DISPLAY_NAME").unwrap_or_default(),
            "session": std::env::var("ATRIUM_SESSION").unwrap_or_default(),
            "user_sid": id.user_sid,
            "logon_session": id.logon_session,
            "pid": std::process::id(),
            "hostname": std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_string()).unwrap_or_default(),
            "browser_session": req.header("x-atrium-browser-session"),
        })),
        ("GET", "/") => asset(SHELL_HTML, "text/html; charset=utf-8"),
        ("GET", "/api/apps") => Response::json(&serde_json::to_value(apps::catalogue()).unwrap_or_default()),
        ("GET", p) if p.starts_with("/apps/") => match apps::resolve(p) {
            Some(file) => match std::fs::read(&file) {
                Ok(body) => Response { status: 200, reason: "OK", content_type: apps::content_type(&file), extra_headers: vec![], body },
                Err(_) => Response::status(404, "Not Found"),
            },
            None => Response::status(404, "Not Found"),
        },
        ("GET", "/shell/shell.css") => asset(SHELL_CSS, "text/css; charset=utf-8"),
        ("GET", "/shell/shell.js") => asset(SHELL_JS, "text/javascript; charset=utf-8"),
        ("GET", "/shell/icons.js") => asset(SHELL_ICONS, "text/javascript; charset=utf-8"),
        ("GET", "/shell/sdk.js") => asset(SHELL_SDK, "text/javascript; charset=utf-8"),
        _ => Response::status(404, "Not Found"),
    };
    write_response(&mut conn, &resp);
    let _ = conn.flush();
}

fn main() -> std::process::ExitCode {
    // SAFETY: fd 3 is the control socket the spawner placed; nothing else
    // in this process refers to it.
    let control = unsafe { UnixStream::from_raw_fd(CONTROL_FD) };
    let id = identity();
    let state: state::Shared = Default::default();
    eprintln!("atrium-session: running as {} in logon session {} (pid {})", id.user_sid, id.logon_session, std::process::id());
    loop {
        let (msg, fd): (SessionMessage, _) = match read_frame_fd(&control) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // The server is gone, and with it every way to reach us.
                eprintln!("atrium-session: server closed the control socket; exiting");
                return std::process::ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("atrium-session: control socket: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        let SessionMessage::Connection { .. } = msg;
        let Some(fd) = fd else {
            eprintln!("atrium-session: connection frame carried no descriptor");
            continue;
        };
        let conn = UnixStream::from(fd);
        let id = Identity { user_sid: id.user_sid.clone(), logon_session: id.logon_session.clone() };
        let state = std::sync::Arc::clone(&state);
        std::thread::spawn(move || serve(conn, &id, &state));
    }
}
