//! atrium-session — one interactive session, as the user.
//!
//! Started with the user's primary token installed and one socketpair on fd
//! 3 whose other end atrium-server holds. It has no privilege and needs
//! none: it is the user. Every browser connection bound to this session
//! arrives as a `SessionMessage::Connection` frame carrying a descriptor,
//! already authenticated, already tagged with which browser it came from.
//! "Accept" here means "receive a descriptor".
//!
//! Slice 3: it answers with a page proving where it runs — the token's user
//! SID, the pid, the browser-session tag the server attached. The shell,
//! apps and everything else grow from here.

use std::io::Write;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;

use atrium_http::{Request, Response, read_request, write_response};
use atrium_proto::{CONTROL_FD, SessionMessage, read_frame_fd};
use peios::token::{Token, TokenAccess};
use serde_json::json;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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

fn page(req: &Request, id: &Identity) -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let browser = req.header("x-atrium-browser-session").unwrap_or("-");
    let addr = req.header("x-atrium-client-addr").unwrap_or("-");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Atrium</title>\
<style>:root{{color-scheme:light dark;font-family:system-ui,sans-serif}}body{{margin:2rem}}dt{{font-weight:600;margin-top:.5rem}}button{{font:inherit}}</style></head>\
<body><h1>Hello {user}</h1>\
<p>This page comes from <code>atrium-session</code> pid {pid}, a process that is you.</p>\
<dl><dt>Token user</dt><dd><code>{sid}</code></dd>\
<dt>Logon session</dt><dd><code>{ls}</code></dd>\
<dt>Browser session</dt><dd><code>{browser}</code> from <code>{addr}</code></dd>\
<dt>Path</dt><dd><code>{path}</code></dd></dl>\
<form method=\"post\" action=\"/api/logout\"><button>Log out</button></form></body></html>",
        user = escape(&user),
        pid = std::process::id(),
        sid = escape(&id.user_sid),
        ls = escape(&id.logon_session),
        browser = escape(browser),
        addr = escape(addr),
        path = escape(&req.path),
    )
}

fn serve(mut conn: UnixStream, id: &Identity) {
    let Some(req) = read_request(&mut conn) else {
        write_response(&mut conn, &Response::status(400, "Bad Request"));
        return;
    };
    let resp = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/whoami") => Response::json(&json!({
            "user": std::env::var("USER").unwrap_or_default(),
            "user_sid": id.user_sid,
            "logon_session": id.logon_session,
            "pid": std::process::id(),
            "browser_session": req.header("x-atrium-browser-session"),
        })),
        ("GET", _) => Response::html(&page(&req, id)),
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
        std::thread::spawn(move || serve(conn, &id));
    }
}
