//! Starting atriumd's children.
//!
//! **The server** is atriumd's own child: create a socketpair, fork, and in
//! the child ask for SIGTERM on parent death, place the child's end on fd 3
//! (`atrium_proto::CONTROL_FD`), install a copy of atriumd's own token with
//! every privilege deleted (`Token::restrict`) — the Chrome sandbox move:
//! same user SID, so nothing about file ownership changes, but no SeTcb,
//! no SeAssignPrimaryToken, nothing a network parser can spend — and exec.
//!
//! **A session host** is not atriumd's child at all. It is a job submitted
//! to peinit (PSPU §7): the logon token authd gave us travels with the
//! `submit` as the job identity, the session's control socket travels as a
//! descriptor, and peinit is the parent — cgroup, output to eventd, the
//! job record, and the one process on the machine that installs primaries
//! for other people. atriumd gets a pidfd back and never forks as anyone
//! but itself.

use std::ffi::CString;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

use libauthd::wire::Profile;
use peios::security::Privileges;
use peios::token::{RestrictSpec, Token, TokenAccess};
use serde_json::json;

use crate::jobs::JobsClient;
use crate::log;

/// Where the binaries live on an image. `ATRIUM_SERVER` / `ATRIUM_SESSION`
/// override them for a checkout.
const SERVER_PATH: &str = "/usr/libexec/atrium/atrium-server";
const SESSION_PATH: &str = "/usr/libexec/atrium/atrium-session";

pub struct Server {
    pub pid: libc::pid_t,
    pub control: UnixStream,
}

pub struct SpawnedSession {
    /// peinit's job identifier: what `stop` and `status` address.
    pub job_id: String,
    /// The job's process handle, so the main loop can poll for its exit.
    pub pidfd: OwnedFd,
    /// The end of the session's control socket that goes to atrium-server.
    pub server_end: OwnedFd,
}

fn server_path() -> String {
    std::env::var("ATRIUM_SERVER").unwrap_or_else(|_| SERVER_PATH.to_string())
}

fn session_path() -> String {
    std::env::var("ATRIUM_SESSION").unwrap_or_else(|_| SESSION_PATH.to_string())
}

/// Build the token the server runs under.
fn restricted_token() -> std::io::Result<Token> {
    // ASSIGN_PRIMARY as well as DUPLICATE: the fd `restrict` returns carries
    // the access mask of the fd it was made from (token_fd.c,
    // pkm_kacs_token_to_fd(new_token, tf->access_mask)), and install needs
    // ASSIGN_PRIMARY on the fd being installed.
    let me = Token::open_self(true, TokenAccess::DUPLICATE | TokenAccess::QUERY | TokenAccess::ASSIGN_PRIMARY)
        .map_err(std::io::Error::other)?;
    let spec = RestrictSpec { privs_to_delete: Privileges::all(), ..RestrictSpec::default() };
    me.restrict(&spec).map_err(std::io::Error::other)
}

fn socketpair() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0i32; 2];
    // SAFETY: plain syscall writing two fds into a 2-array.
    if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both fds are freshly created and owned by nobody else.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Exit codes the server child uses to say what went wrong before exec.
const EXIT_INSTALL: i32 = 125;
const EXIT_FD: i32 = 126;
const EXIT_EXEC: i32 = 127;

pub fn spawn_server(unrestricted: bool) -> std::io::Result<Server> {
    let (ours, theirs) = socketpair()?;
    // Everything the child needs, prepared before the fork: after fork only
    // async-signal-safe work happens until exec (atriumd is single-threaded
    // here, so this is belt and braces).
    let token = if unrestricted { None } else { Some(restricted_token()?) };
    let path = CString::new(server_path()).map_err(std::io::Error::other)?;
    let argv0 = c"atrium-server";

    // SAFETY: fork in a single-threaded process; the child does only
    // syscalls and exec.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if pid == 0 {
        // Child. Nothing below returns.
        // SAFETY: syscalls on fds and strings we own; _exit on any failure.
        unsafe {
            // Must not outlive atriumd: with it gone nothing answers the
            // control socket and every cookie is orphaned.
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            // dup2 clears CLOEXEC on the copy, which is the point.
            if libc::dup2(theirs.as_raw_fd(), atrium_proto::CONTROL_FD) < 0 {
                libc::_exit(EXIT_FD);
            }
            if let Some(t) = &token {
                if t.install().is_err() {
                    libc::_exit(EXIT_INSTALL);
                }
            }
            let argv = [argv0.as_ptr(), std::ptr::null()];
            libc::execv(path.as_ptr(), argv.as_ptr());
            libc::_exit(EXIT_EXEC);
        }
    }
    drop(theirs);
    log::info(format_args!("atrium-server started (pid {pid})"));
    Ok(Server { pid, control: UnixStream::from(ours) })
}

/// Submit a session host to peinit as `username`, identified by `token`
/// (the logon primary; `None` under `--unrestricted`, when the job runs as
/// atriumd itself).
pub fn spawn_session(
    jobs: &mut JobsClient,
    session: u64,
    token: Option<&Token>,
    username: &str,
    profile: &Profile,
) -> std::io::Result<SpawnedSession> {
    let (server_end, theirs) = socketpair()?;
    // The environment `login` gives a shell, minus the shell's own; peinit
    // sets none of these (PSPU §7.6) because it knows nothing but the
    // token. A missing home is not a failed session: `login` starts in /
    // for the same reason, and peinit would fail the job on a chdir it
    // cannot make.
    let home = if profile.home.starts_with('/') && std::path::Path::new(&profile.home).is_dir() {
        profile.home.as_str()
    } else {
        "/"
    };
    let display_name = if profile.display_name.is_empty() { username } else { profile.display_name.as_str() };
    let shell = if profile.shell.starts_with('/') { profile.shell.as_str() } else { "/bin/sh" };
    let definition = json!({
        "image_path": session_path(),
        "arguments": [],
        "environment": {
            "HOME": home,
            "USER": username,
            "LOGNAME": username,
            "PATH": "/usr/bin:/bin",
            "ATRIUM_SESSION": session.to_string(),
            "ATRIUM_DISPLAY_NAME": display_name,
            "SHELL": shell,
        },
        "working_directory": home,
        "description": format!("Atrium session for {username}"),
        "descriptors": ["atrium-control"],
        "stop_timeout": 10,
    });
    let submitted = jobs.submit(definition, token.map(|t| t.as_fd()), &[theirs.as_fd()])?;
    drop(theirs);
    if submitted.state != "running" {
        return Err(std::io::Error::other(format!(
            "job {} is {}{}",
            submitted.id,
            submitted.state,
            submitted.cause.map(|c| format!(" ({c})")).unwrap_or_default()
        )));
    }
    let Some(pidfd) = submitted.pidfd else {
        return Err(std::io::Error::other(format!("job {} is running but peinit sent no process handle", submitted.id)));
    };
    log::info(format_args!("atrium-session started for {username} (session {session}, job {})", submitted.id));
    Ok(SpawnedSession { job_id: submitted.id, pidfd, server_end })
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
            EXIT_INSTALL => "could not install its token".into(),
            EXIT_FD => "could not place the control socket".into(),
            EXIT_EXEC => "could not exec".into(),
            0 => "exited".into(),
            n => format!("exit code {n}"),
        }
    } else if libc::WIFSIGNALED(status) {
        format!("signal {}", libc::WTERMSIG(status))
    } else {
        "unknown".into()
    }
}

/// Ask peinit to end a session host: SIGTERM, then its kill after the
/// stop timeout.
pub fn terminate(jobs: &mut JobsClient, job_id: &str) {
    if let Err(e) = jobs.stop(job_id) {
        log::warn(format_args!("stop job {job_id}: {e}"));
    }
}
