//! Spawning atriumd's children: the server, and — for now — session hosts.
//!
//! Both follow the same shape: create a socketpair, fork, and in the child
//! ask for SIGTERM on parent death, place the child's end on fd 3
//! (`atrium_proto::CONTROL_FD`), install a token, exec. Every other
//! descriptor is CLOEXEC.
//!
//! The server gets a copy of atriumd's own token with every privilege
//! deleted (`Token::restrict`) — the Chrome sandbox move: same user SID, so
//! nothing about file ownership changes, but no SeTcb, no
//! SeAssignPrimaryToken, nothing a network parser can spend.
//!
//! A session host gets the user's primary token from the logon, and is the
//! user from its first instruction. **This half is a stopgap**: it is what
//! peinit's jobs API will do instead (PEI-523), with cgroups, output
//! routing and job records that are deliberately not reimplemented here.
//! atriumd being a process factory for users is the thing that goes away.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

use libauthd::wire::Profile;
use peios::security::Privileges;
use peios::token::{RestrictSpec, Token, TokenAccess};

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
    pub pid: libc::pid_t,
    /// A pidfd, so the main loop can poll for the session's death.
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

/// What the child does between fork and exec. Everything is prepared before
/// the fork; after it only async-signal-safe calls happen (atriumd is
/// single-threaded, so this is belt and braces).
struct Child<'a> {
    control: &'a OwnedFd,
    token: Option<&'a Token>,
    /// `setsid` and `chdir` here: a session host is a session leader in
    /// its own process group (so ending it ends its tree) and starts in the
    /// user's home. The server is neither.
    session: Option<&'a CString>,
    path: &'a CString,
    argv0: &'a CString,
    envp: Option<&'a [CString]>,
}

/// Exit codes the child uses to say what went wrong before exec.
const EXIT_INSTALL: i32 = 125;
const EXIT_FD: i32 = 126;
const EXIT_EXEC: i32 = 127;

fn fork_child(c: &Child<'_>) -> std::io::Result<libc::pid_t> {
    // SAFETY: fork in a single-threaded process; the child does only
    // syscalls and exec.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if pid != 0 {
        return Ok(pid);
    }
    // Child. Nothing below returns.
    // SAFETY: syscalls on fds and strings we own; _exit on any failure.
    unsafe {
        // Must not outlive atriumd: with it gone nothing answers a control
        // socket and every cookie is orphaned.
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
        // dup2 clears CLOEXEC on the copy, which is the point.
        if libc::dup2(c.control.as_raw_fd(), atrium_proto::CONTROL_FD) < 0 {
            libc::_exit(EXIT_FD);
        }
        if let Some(t) = c.token {
            if t.install().is_err() {
                libc::_exit(EXIT_INSTALL);
            }
        }
        if let Some(home) = c.session {
            libc::setsid();
            if libc::chdir(home.as_ptr()) != 0 {
                // A missing home is not a failed session; `login` starts
                // in / for the same reason.
                libc::chdir(c"/".as_ptr());
            }
        }
        let argv = [c.argv0.as_ptr(), std::ptr::null()];
        match c.envp {
            Some(env) => {
                let mut envp: Vec<*const libc::c_char> = env.iter().map(|e| e.as_ptr()).collect();
                envp.push(std::ptr::null());
                libc::execve(c.path.as_ptr(), argv.as_ptr(), envp.as_ptr());
            }
            None => {
                libc::execv(c.path.as_ptr(), argv.as_ptr());
            }
        }
        libc::_exit(EXIT_EXEC);
    }
}

pub fn spawn_server(unrestricted: bool) -> std::io::Result<Server> {
    let (ours, theirs) = socketpair()?;
    let token = if unrestricted { None } else { Some(restricted_token()?) };
    let path = CString::new(server_path()).map_err(std::io::Error::other)?;
    let argv0 = c"atrium-server".to_owned();
    let pid = fork_child(&Child { control: &theirs, token: token.as_ref(), session: None, path: &path, argv0: &argv0, envp: None })?;
    drop(theirs);
    log::info(format_args!("atrium-server started (pid {pid})"));
    Ok(Server { pid, control: UnixStream::from(ours) })
}

pub fn spawn_session(session: u64, token: Option<&Token>, username: &str, profile: &Profile) -> std::io::Result<SpawnedSession> {
    let (server_end, theirs) = socketpair()?;
    let path = CString::new(session_path()).map_err(std::io::Error::other)?;
    let argv0 = c"atrium-session".to_owned();
    // The environment `login` gives a shell, minus the shell's own. A
    // relative home is refused for the reason login refuses one: nothing
    // should resolve relative paths as the user before the user's code runs.
    let home = if profile.home.starts_with('/') { profile.home.as_str() } else { "/" };
    let env: Vec<CString> = [
        format!("HOME={home}"),
        format!("USER={username}"),
        format!("LOGNAME={username}"),
        "PATH=/usr/bin:/bin".to_string(),
        format!("ATRIUM_SESSION={session}"),
        format!("ATRIUM_DISPLAY_NAME={}", if profile.display_name.is_empty() { username } else { profile.display_name.as_str() }),
    ]
    .into_iter()
    .filter_map(|s| CString::new(s).ok())
    .collect();
    let home_c = CString::new(home).map_err(std::io::Error::other)?;
    let pid = fork_child(&Child { control: &theirs, token, session: Some(&home_c), path: &path, argv0: &argv0, envp: Some(&env) })?;
    drop(theirs);
    // SAFETY: pidfd_open on our own just-forked child; the fd is ours.
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if raw < 0 {
        let e = std::io::Error::last_os_error();
        // SAFETY: killing the child we just made.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        return Err(e);
    }
    // SAFETY: a fresh fd from the syscall above.
    let pidfd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
    log::info(format_args!("atrium-session started for {username} (session {session}, pid {pid})"));
    Ok(SpawnedSession { pid, pidfd, server_end })
}

/// Collect a child's exit status. Blocks; call once the pidfd is readable
/// or the control socket has closed.
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

/// End a session host's whole process group.
pub fn terminate(pid: libc::pid_t) {
    // SAFETY: signalling a process group we created with setsid.
    unsafe { libc::kill(-pid, libc::SIGTERM) };
}
