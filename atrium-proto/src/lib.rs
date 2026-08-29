//! The control protocol between atriumd and atrium-server.
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

use serde::{Deserialize, Serialize};

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
    /// Logged on. atriumd holds the token; the server may now bind a cookie
    /// to `session`.
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
}
