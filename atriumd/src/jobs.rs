//! A client for peinit's jobs channel (PSPU §7), as much of it as atriumd
//! needs: submit, status, stop.
//!
//! One `SOCK_SEQPACKET` connection to `/run/services/peinit/jobs.sock`, one
//! JSON object per message. A `submit` carries the job identity as a
//! `KACS_SCM_TOKEN` (the logon primary authd gave us — the kernel gates the
//! attach, peinit duplicates it to the job's primary) and the session's
//! control socket as an `SCM_RIGHTS` descriptor, which the job finds at fd
//! 3. The response to a running job carries its pidfd.
//!
//! atriumd never installs a token and never forks a user process; peinit
//! is the parent, and holds the record.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, FromRawFd, OwnedFd};

use serde_json::{Value, json};

pub const JOBS_SOCKET_PATH: &str = "/run/services/peinit/jobs.sock";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

pub struct JobsClient {
    sock: OwnedFd,
}

pub struct Submitted {
    pub id: String,
    /// Present when the job is running.
    pub pidfd: Option<OwnedFd>,
    pub state: String,
    pub cause: Option<String>,
}

fn jobs_socket_path() -> String {
    std::env::var("ATRIUM_JOBS_SOCKET").unwrap_or_else(|_| JOBS_SOCKET_PATH.to_string())
}

impl JobsClient {
    pub fn connect() -> io::Result<JobsClient> {
        let path = jobs_socket_path();
        // std has no SOCK_SEQPACKET UnixStream; build the socket by hand.
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = path.as_bytes();
        if bytes.len() >= addr.sun_path.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "jobs socket path too long"));
        }
        for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
            *dst = *src as libc::c_char;
        }
        // SAFETY: plain socket/connect syscalls on a sockaddr we filled.
        let sock = unsafe {
            let fd = libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let sock = OwnedFd::from_raw_fd(fd);
            let len = (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t;
            if libc::connect(fd, (&addr as *const libc::sockaddr_un).cast(), len) < 0 {
                return Err(io::Error::last_os_error());
            }
            sock
        };
        Ok(JobsClient { sock })
    }

    fn request(&mut self, request: &Value, token: Option<BorrowedFd<'_>>, fds: &[BorrowedFd<'_>]) -> io::Result<(Value, Option<OwnedFd>)> {
        let bytes = serde_json::to_vec(request)?;
        peios::socket::send_message(self.sock.as_fd(), &bytes, token, fds, 0).map_err(io::Error::from)?;
        let mut buf = vec![0u8; MAX_RESPONSE_BYTES];
        let got = peios::socket::recv_message(self.sock.as_fd(), &mut buf, 1, 0).map_err(io::Error::from)?;
        if got.len == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "jobs socket closed"));
        }
        if got.truncated {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "jobs response too large"));
        }
        buf.truncate(got.len);
        let value: Value = serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut fds = got.fds;
        let fd = fds.pop();
        match value.get("status").and_then(Value::as_str) {
            Some("ok") => Ok((value, fd)),
            Some("error") => {
                let code = value.get("code").and_then(Value::as_str).unwrap_or("?");
                let message = value.get("message").and_then(Value::as_str).unwrap_or("");
                Err(io::Error::other(format!("peinit refused: {code}: {message}")))
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "jobs response has no status")),
        }
    }

    pub fn submit(&mut self, mut definition: Value, token: Option<BorrowedFd<'_>>, fds: &[BorrowedFd<'_>]) -> io::Result<Submitted> {
        definition["command"] = json!("submit");
        let (value, pidfd) = self.request(&definition, token, fds)?;
        let job = value.get("job").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "submit answered without a job"))?;
        let id = job.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let state = job.get("state").and_then(Value::as_str).unwrap_or("?").to_string();
        let cause = job.get("cause").and_then(Value::as_str).map(str::to_string);
        Ok(Submitted { id, pidfd, state, cause })
    }

    /// The job view, as JSON.
    pub fn status(&mut self, id: &str) -> io::Result<Value> {
        let (value, _) = self.request(&json!({"command": "status", "job_id": id}), None, &[])?;
        Ok(value.get("job").cloned().unwrap_or(Value::Null))
    }

    /// SIGTERM, then peinit's kill after the job's stop timeout.
    pub fn stop(&mut self, id: &str) -> io::Result<()> {
        self.request(&json!({"command": "stop", "job_id": id, "wait": false}), None, &[]).map(|_| ())
    }
}

/// One line describing how a job ended, from its view.
pub fn describe_end(view: &Value) -> String {
    let state = view.get("state").and_then(Value::as_str).unwrap_or("?");
    let cause = view.get("cause").and_then(Value::as_str);
    let code = view.get("exit_code").and_then(Value::as_i64);
    let signal = view.get("exit_signal").and_then(Value::as_i64);
    let mut s = state.to_string();
    if let Some(c) = cause {
        s.push_str(&format!(", {c}"));
    }
    if let Some(c) = code {
        s.push_str(&format!(", exit code {c}"));
    }
    if let Some(sig) = signal {
        s.push_str(&format!(", signal {sig}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    /// Against a fake manager on `ATRIUM_JOBS_SOCKET` (tmp/fake_peinit.py):
    /// submit a job with one descriptor, get a running view and a pidfd,
    /// talk to the job over the descriptor, stop it, see it end.
    #[test]
    fn submit_stop_against_fake_manager() {
        if std::env::var("ATRIUM_JOBS_SOCKET").is_err() {
            eprintln!("ATRIUM_JOBS_SOCKET unset; skipping");
            return;
        }
        let mut jobs = JobsClient::connect().expect("connect");
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        let def = json!({
            "image_path": "/bin/sh",
            "arguments": ["-c", "read line <&3; echo \"got $line\" >&3; exec sleep 30"],
            "environment": {"X": "1"},
            "working_directory": "/",
            "description": "test",
            "descriptors": ["ctl"],
            "stop_timeout": 2,
        });
        let sub = jobs.submit(def, None, &[theirs.as_fd()]).expect("submit");
        drop(theirs);
        assert_eq!(sub.state, "running");
        let pidfd = sub.pidfd.expect("pidfd");
        ours.write_all(b"hello\n").unwrap();
        let mut buf = [0u8; 32];
        let n = ours.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"got hello\n");
        jobs.stop(&sub.id).expect("stop");
        let mut pfd = libc::pollfd { fd: pidfd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let rc = unsafe { libc::poll(&mut pfd, 1, 5000) };
        assert_eq!(rc, 1, "job did not exit after stop");
        let view = jobs.status(&sub.id).expect("status");
        let how = describe_end(&view);
        assert!(how.starts_with("failed"), "{how}");
    }
}
