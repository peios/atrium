//! The least of RFC 6455 the shell's mirror needs: accept an upgrade, read
//! client frames (masked, text/close/ping), write unmasked text frames.
//!
//! No fragmentation, no extensions, no binary. The session's channel to
//! the shell is small JSON messages; anything larger is an app's business
//! and goes over its own channel.

use std::io::{self, Read, Write};

use atrium_http::Request;
use sha1::{Digest, Sha1};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_PAYLOAD: usize = 256 * 1024;

pub const TEXT: u8 = 0x1;
pub const CLOSE: u8 = 0x8;
pub const PING: u8 = 0x9;
pub const PONG: u8 = 0xA;

pub fn is_upgrade(req: &Request) -> bool {
    req.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket")) && req.header("sec-websocket-key").is_some()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Complete the handshake on `w`. After this the stream carries frames.
pub fn accept<W: Write>(w: &mut W, req: &Request) -> io::Result<()> {
    let key = req.header("sec-websocket-key").unwrap_or_default();
    let digest = Sha1::digest(format!("{key}{GUID}").as_bytes());
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        base64(&digest)
    );
    w.write_all(head.as_bytes())?;
    w.flush()
}

pub struct Frame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// One frame from the client, or `None` at EOF or on a malformed frame.
pub fn read_frame<R: Read>(r: &mut R) -> Option<Frame> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h).ok()?;
    let opcode = h[0] & 0x0F;
    let masked = h[1] & 0x80 != 0;
    let mut len = usize::from(h[1] & 0x7F);
    if len == 126 {
        let mut b = [0u8; 2];
        r.read_exact(&mut b).ok()?;
        len = usize::from(u16::from_be_bytes(b));
    } else if len == 127 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b).ok()?;
        len = usize::try_from(u64::from_be_bytes(b)).ok()?;
    }
    if len > MAX_PAYLOAD || !masked {
        // Clients must mask (RFC 6455 §5.1); an unmasked frame is a
        // protocol error and the connection ends.
        return None;
    }
    let mut mask = [0u8; 4];
    r.read_exact(&mut mask).ok()?;
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).ok()?;
    for (i, b) in payload.iter_mut().enumerate() {
        *b ^= mask[i % 4];
    }
    Some(Frame { opcode, payload })
}

pub fn write_frame<W: Write>(w: &mut W, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut head = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => head.push(n as u8),
        n if n <= 0xFFFF => {
            head.push(126);
            head.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            head.push(127);
            head.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    w.write_all(&head)?;
    w.write_all(payload)?;
    w.flush()
}

pub fn send_text<W: Write>(w: &mut W, text: &str) -> io::Result<()> {
    write_frame(w, TEXT, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_key_matches_rfc_example() {
        // RFC 6455 §1.3.
        let digest = Sha1::digest(format!("dGhlIHNhbXBsZSBub25jZQ=={GUID}").as_bytes());
        assert_eq!(base64(&digest), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn masked_frame_round_trip() {
        let payload = b"hello";
        let mask = [1u8, 2, 3, 4];
        let mut bytes = vec![0x81, 0x80 | payload.len() as u8];
        bytes.extend_from_slice(&mask);
        bytes.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        let f = read_frame(&mut &bytes[..]).unwrap();
        assert_eq!(f.opcode, TEXT);
        assert_eq!(f.payload, payload);
    }
}
