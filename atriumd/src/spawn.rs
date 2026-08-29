//! Spawning atrium-server: fork, strip privilege, hand over the control
//! socket, exec.
//!
//! The server is the network-facing half and must never hold what atriumd
//! holds. It gets a copy of atriumd's own token with every privilege deleted
//! (`Token::restrict`), installed in the child before exec — the Chrome
//! sandbox move. Same user SID, so nothing about file ownership changes, but
//! no SeTcb, no SeAssignPrimaryToken, nothing a network parser can spend.
//!
//! The control socket rides as fd 3 (`atrium_proto::CONTROL_FD`). Every
//! other descriptor is CLOEXEC or explicitly closed.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

use peios::security::Privileges;
use peios::token::{RestrictSpec, Token, TokenAccess};

use crate::log;

/// Where the server binary lives on an image. `ATRIUM_SERVER` overrides it
/// for a checkout.
const SERVER_PATH: &str = "/usr/libexec/atrium/atrium-server";

pub struct Server {
    pub pid: libc::pid_t,
    pub control: UnixStream,
}

fn server_path() -> String {
    std::env::var("ATRIUM_SERVER").unwrap_or_else(|_| SERVER_PATH.to_string())
}

/// Build the token the server runs under. `None` under `--unrestricted`, for
/// a host with no KACS.
fn restricted_token() -> std::io::Result<Token> {
    let me = Token::open_self(true, TokenAccess::DUPLICATE | TokenAccess::QUERY).map_err(std::io::Error::other)?;
    let spec = RestrictSpec { privs_to_delete: Privileges::all(), ..RestrictSpec::default() };
    me.restrict(&spec).map_err(std::io::Error::other)
}

pub fn spawn(unrestricted: bool) -> std::io::Result<Server> {
    let mut fds = [0i32; 2];
    // SAFETY: plain syscall writing two fds into a 2-array.
    if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both fds are freshly created and owned by nobody else.
    let (ours, theirs) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };

    // Everything the child needs, prepared before the fork: after fork only
    // async-signal-safe work happens until exec, and atriumd is single-threaded
    // at this point so even that is belt and braces.
    let token = if unrestricted { None } else { Some(restricted_token()?) };
    let path = CString::new(server_path()).map_err(std::io::Error::other)?;
    let argv0 = CString::new("atrium-server").unwrap();

    // SAFETY: fork in a single-threaded process; the child does dup2, token
    // install, and exec.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if pid == 0 {
        // Child. The server must not outlive atriumd: with atriumd gone
        // nothing answers its control socket and every cookie it holds is
        // orphaned, so ask the kernel to SIGTERM it the moment its parent
        // dies. dup2 clears CLOEXEC on the copy, which is the point.
        // SAFETY: syscalls on fds we own.
        unsafe {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            if libc::dup2(theirs.as_raw_fd(), atrium_proto::CONTROL_FD) < 0 {
                libc::_exit(126);
            }
        }
        if let Some(t) = &token {
            if t.install().is_err() {
                // SAFETY: nothing to unwind in a forked child.
                unsafe { libc::_exit(125) };
            }
        }
        let argv = [argv0.as_ptr(), std::ptr::null()];
        // SAFETY: NUL-terminated strings and a NULL-terminated argv.
        unsafe {
            libc::execv(path.as_ptr(), argv.as_ptr());
            libc::_exit(127);
        }
    }
    drop(theirs);
    log::info(format_args!("atrium-server started (pid {pid})"));
    Ok(Server { pid, control: UnixStream::from(ours) })
}

/// Collect the server's exit status once its control socket has closed.
pub fn reap(pid: libc::pid_t) -> String {
    let mut status = 0;
    // SAFETY: waitpid on our own child.
    if unsafe { libc::waitpid(pid, &mut status, 0) } != pid {
        return "unknown".into();
    }
    if libc::WIFEXITED(status) {
        match libc::WEXITSTATUS(status) {
            125 => "could not install its restricted token".into(),
            126 => "could not place the control socket".into(),
            127 => format!("could not exec {}", server_path()),
            n => format!("exit code {n}"),
        }
    } else if libc::WIFSIGNALED(status) {
        format!("signal {}", libc::WTERMSIG(status))
    } else {
        "unknown".into()
    }
}
