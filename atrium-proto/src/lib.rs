//! The control protocols inside Atrium.
//!
//! Two of them, one framing:
//!
//!   * atriumd <-> atrium-server, on the socketpair atriumd forked the server
//!     with (`Request`/`Reply`);
//!   * atrium-server -> a session host, on the socketpair the session was
//!     started with (`SessionMessage`), carrying one forwarded connection
//!     per frame.
//!
//! Descriptors travel as `SCM_RIGHTS` attached to a frame's length prefix
//! (`fdpass`); which frames carry one is documented on the message.
//!
//! One socketpair, created by atriumd before it forks the server, carries it.
//! Trust is structural — nothing else holds either end — so there is no peer
//! verification and no authentication in the messages.
//!
//! Framing is a `u32` little-endian length followed by one JSON document.
//! JSON rather than a hand codec because both ends are ours, the volume is a
//! few messages per login, and readable frames are worth more here than
//! compact ones. Credential answers cross this link in the clear, as they
//! cross the logon socket (PGSS Logon §12) — the link is a socketpair inside
//! one process tree.
//!
//! The server speaks first and every request gets exactly one reply, in
//! order. Logon conversations are identified by a `conv` the server chooses;
//! it is opaque to atriumd beyond matching a `LogonAnswer` to its `LogonStart`.

use std::io::{self, Read, Write};
use std::os::fd::{BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

pub mod fdpass;

/// The fd number the server finds its control socket on at exec.
pub const CONTROL_FD: i32 = 3;

/// Largest frame either side will accept.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// Server -> atriumd.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Begin a logon conversation for `username` from `remote`.
    LogonStart { conv: u64, username: String, remote: String },
    /// Answer the prompts of the last `Prompt` reply for this conversation.
    LogonAnswer { conv: u64, answers: Vec<Answer> },
    /// Drop a conversation that will not be continued.
    LogonAbort { conv: u64 },
    /// The browser side has logged out of this session: release its token.
    Logout { session: u64 },
}

/// atriumd -> server.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    /// The authority wants credentials. Render the messages, collect the
    /// prompts, send `LogonAnswer`.
    Prompt { conv: u64, messages: Vec<Message>, prompts: Vec<Prompt> },
    /// Logged on and a session host started. atriumd holds the token; the
    /// server may now bind a cookie to `session`. **Carries a descriptor**:
    /// the server's end of the session host's control socket, to send
    /// `SessionMessage`s down.
    Granted { conv: u64, session: u64, username: String, display_name: String },
    /// Refused. `retryable` says whether the same user may try again on this
    /// page or the page should reset.
    Denied { conv: u64, retryable: bool, reason: String },
    /// The request could not be carried out (transport to the authority
    /// failed, unknown conversation). Not a logon outcome.
    Error { conv: u64, reason: String },
    /// Acknowledgement of a request with no other outcome.
    Ok,
}

/// atrium-server -> session host.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionMessage {
    /// A browser connection, authenticated and already bound to this
    /// session. **Carries a descriptor**: one end of a socketpair the server
    /// pumps the connection's bytes through. The HTTP on it has had the
    /// cookie stripped and `X-Atrium-Browser-Session` /
    /// `X-Atrium-Client-Addr` added; the fields here repeat those for
    /// convenience.
    Connection { browser_session: String, client_addr: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    /// `info`, `warning` or `error`.
    pub severity: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Prompt {
    /// Opaque; echoed back in the answer.
    pub credential_ref: u32,
    /// `password` is the only type today (PGSS Logon §11).
    pub credential_type: String,
    /// What to label the field.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Answer {
    pub credential_ref: u32,
    pub data: String,
}

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, value: &T) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let len = (body.len() as u32).to_le_bytes();
    w.write_all(&len)?;
    w.write_all(&body)?;
    w.flush()
}

pub fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// `write_frame` with a descriptor attached (see `fdpass`).
pub fn write_frame_fd<T: Serialize>(sock: &UnixStream, value: &T, fd: Option<BorrowedFd<'_>>) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let len = (body.len() as u32).to_le_bytes();
    fdpass::send_with_fd(sock, &len, fd)?;
    (&mut &*sock).write_all(&body)
}

/// `read_frame` collecting a descriptor if the frame carried one.
pub fn read_frame_fd<T: for<'de> Deserialize<'de>>(sock: &UnixStream) -> io::Result<(T, Option<OwnedFd>)> {
    let mut len = [0u8; 4];
    let fd = fdpass::recv_exact_with_fd(sock, &mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; len];
    (&mut &*sock).read_exact(&mut body)?;
    let value = serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok((value, fd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Request::LogonStart { conv: 7, username: "jack".into(), remote: "10.0.2.2".into() }).unwrap();
        let back: Request = read_frame(&mut &buf[..]).unwrap();
        match back {
            Request::LogonStart { conv, username, remote } => {
                assert_eq!((conv, username.as_str(), remote.as_str()), (7, "jack", "10.0.2.2"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn frame_carries_a_descriptor() {
        use std::io::{Read, Write};
        use std::os::fd::AsFd;
        let (a, b) = UnixStream::pair().unwrap();
        let (mut x, y) = UnixStream::pair().unwrap();
        write_frame_fd(&a, &SessionMessage::Connection { browser_session: "b1".into(), client_addr: "::1".into() }, Some(y.as_fd())).unwrap();
        drop(y);
        let (msg, fd): (SessionMessage, Option<OwnedFd>) = read_frame_fd(&b).unwrap();
        let SessionMessage::Connection { browser_session, .. } = msg;
        assert_eq!(browser_session, "b1");
        let mut received = UnixStream::from(fd.expect("descriptor"));
        x.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        received.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
    }
}
