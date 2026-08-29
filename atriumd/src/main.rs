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
//!   * submit one `atrium-session` per logon to peinit as that principal
//!     (PSPU §7), and hand the server the socket to reach it.
//!
//! It never parses HTTP, never impersonates, and never forks as anyone but
//! itself. If a feature seems to need adding here, it belongs in the server
//! or the session host.

mod jobs;
mod log;
mod logon;
mod spawn;

use std::io::ErrorKind;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixDatagram;
use std::process::ExitCode;

use atrium_proto::{Reply, Request, read_frame, write_frame_fd};

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
        log::warn(format_args!("--unrestricted: the server and every session run as this process's own identity"));
    }

    let server = match spawn::spawn_server(unrestricted) {
        Ok(s) => s,
        Err(e) => {
            log::error(format_args!("could not start atrium-server: {e}"));
            return ExitCode::FAILURE;
        }
    };
    notify_ready();

    let mut logon = logon::Logon::new(unrestricted);
    loop {
        // One poll over the server's control socket and every session's
        // pidfd. Frames are small and the server writes them whole, so a
        // readable control socket is followed by a blocking frame read.
        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(1 + logon.sessions.len());
        fds.push(libc::pollfd { fd: server.control.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        let mut order: Vec<u64> = Vec::with_capacity(logon.sessions.len());
        for (id, s) in &logon.sessions {
            fds.push(libc::pollfd { fd: s.pidfd.as_raw_fd(), events: libc::POLLIN, revents: 0 });
            order.push(*id);
        }
        // SAFETY: poll over fds we own, for their lifetime.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == ErrorKind::Interrupted {
                continue;
            }
            log::error(format_args!("poll: {e}"));
            return ExitCode::FAILURE;
        }

        // Dead sessions first, so a Logout for one is not answered after
        // its reap.
        for (i, id) in order.iter().enumerate() {
            if fds[i + 1].revents != 0 {
                logon.ended(*id);
            }
        }

        if fds[0].revents == 0 {
            continue;
        }
        let request: Request = match read_frame(&mut &server.control) {
            Ok(r) => r,
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                // The server is gone; so is every cookie it held, and every
                // session host will follow (its control socket just closed).
                // Exit and let peinit restart the pair — sessions do not
                // survive this today, by decision.
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
        let fd = logon.pending_fd.take();
        if let Err(e) = write_frame_fd(&server.control, &reply, fd.as_ref().map(|f| f.as_fd())) {
            log::error(format_args!("control socket: {e}"));
            return ExitCode::FAILURE;
        }
    }
}
