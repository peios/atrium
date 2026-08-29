//! atriumd — the Peios web session host.
//!
//! The one privileged process in Atrium, and deliberately the smallest. It
//! runs as SYSTEM because that is what originating a logon takes today
//! (`/run/logon.sock` admits SYSTEM alone), and everything it does is on the
//! far side of that fact:
//!
//!   * spawn `atrium-server`, the network-facing half, under a token with
//!     every privilege deleted, joined to us by one socketpair;
//!   * on the server's behalf, hold logon conversations with the authority
//!     (PGSS Logon) and keep the resulting tokens in a session table;
//!   * eventually, ask peinit to start one `atrium-session` per logon as that
//!     principal. That step waits on peinit's jobs API; today a session is a
//!     held token and a log line.
//!
//! It never parses HTTP and never impersonates. If a feature seems to need
//! adding here, it belongs in the server or the session host.

mod log;
mod logon;
mod spawn;

use std::io::ErrorKind;
use std::os::unix::net::UnixDatagram;
use std::process::ExitCode;

use atrium_proto::{Reply, Request, read_frame, write_frame};

fn notify_ready() {
    let Ok(path) = std::env::var("NOTIFY_SOCKET") else { return };
    match UnixDatagram::unbound() {
        Ok(s) => {
            if let Err(e) = s.send_to(b"READY=1", &path) {
                log::warn(format_args!("readiness notify: {e}"));
            }
        }
        Err(e) => log::warn(format_args!("readiness notify: {e}")),
    }
}

fn main() -> ExitCode {
    // `--unrestricted` skips the token work, for a host without KACS. It is
    // a development switch and the log says so every time.
    let unrestricted = std::env::args().any(|a| a == "--unrestricted");
    if unrestricted {
        log::warn(format_args!("--unrestricted: atrium-server runs with this process's own token"));
    }

    let mut server = match spawn::spawn(unrestricted) {
        Ok(s) => s,
        Err(e) => {
            log::error(format_args!("could not start atrium-server: {e}"));
            return ExitCode::FAILURE;
        }
    };
    notify_ready();

    let mut logon = logon::Logon::default();
    loop {
        let request: Request = match read_frame(&mut server.control) {
            Ok(r) => r,
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                // The server is gone; so is every cookie it held. Exit and
                // let peinit restart the pair — sessions do not survive this
                // today, by decision.
                log::error(format_args!("atrium-server exited: {}", spawn::reap(server.pid)));
                return ExitCode::FAILURE;
            }
            Err(e) => {
                log::error(format_args!("control socket: {e}"));
                return ExitCode::FAILURE;
            }
        };
        let reply = match request {
            Request::LogonStart { conv, username, remote } => logon.start(conv, username, remote),
            Request::LogonAnswer { conv, answers } => logon.answer(conv, answers),
            Request::LogonAbort { conv } => logon.abort(conv),
            Request::Logout { session } => logon.logout(session),
        };
        if let Reply::Error { reason, .. } = &reply {
            log::warn(format_args!("logon step failed: {reason}"));
        }
        if let Err(e) = write_frame(&mut server.control, &reply) {
            log::error(format_args!("control socket: {e}"));
            return ExitCode::FAILURE;
        }
    }
}
