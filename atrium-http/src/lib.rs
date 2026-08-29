//! The least HTTP/1.1 that serves a page and a JSON API.
//!
//! One request per connection (`Connection: close`), a bounded head, a
//! bounded body. This is not a general server and is not trying to be; it is
//! the door the login page, the API and the session hosts sit behind until
//! Atrium has a real one, and everything above it is written against
//! `Request`/`Response` so the door can be swapped. Generic over the stream
//! because atrium-server reads TCP and a session host reads a Unix socket
//! the server forwarded.

use std::collections::HashMap;
use std::io::{Read, Write};

const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    /// Header names lowercased.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// The request line and headers exactly as received, up to and
    /// including the blank line — what a forwarder replays.
    pub raw_head: Vec<u8>,
}

impl Request {
    /// The head again, with `drop` headers removed and `add` lines appended.
    /// For a forwarder that must not pass the bearer through and wants to
    /// say who the browser is.
    pub fn rewritten_head(&self, drop: &[&str], add: &[String]) -> Vec<u8> {
        let head = String::from_utf8_lossy(&self.raw_head);
        let mut out = String::new();
        let mut lines = head.trim_end_matches("\r\n").split("\r\n");
        if let Some(rl) = lines.next() {
            out.push_str(rl);
            out.push_str("\r\n");
        }
        for line in lines {
            let name = line.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
            if !drop.iter().any(|d| *d == name) {
                out.push_str(line);
                out.push_str("\r\n");
            }
        }
        for a in add {
            out.push_str(a);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        out.into_bytes()
    }
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    /// The value of one cookie, if the request carries it.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?
            .split(';')
            .map(str::trim)
            .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
    }
}

pub struct Response {
    pub status: u16,
    pub reason: &'static str,
    pub content_type: &'static str,
    pub extra_headers: Vec<String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn html(body: &str) -> Response {
        Response { status: 200, reason: "OK", content_type: "text/html; charset=utf-8", extra_headers: vec![], body: body.as_bytes().to_vec() }
    }
    pub fn json(value: &serde_json::Value) -> Response {
        Response { status: 200, reason: "OK", content_type: "application/json", extra_headers: vec![], body: value.to_string().into_bytes() }
    }
    pub fn status(status: u16, reason: &'static str) -> Response {
        Response { status, reason, content_type: "text/plain; charset=utf-8", extra_headers: vec![], body: reason.as_bytes().to_vec() }
    }
    pub fn with_header(mut self, h: String) -> Response {
        self.extra_headers.push(h);
        self
    }
}

pub fn read_request<S: Read>(stream: &mut S) -> Option<Request> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() >= MAX_HEAD_BYTES {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return None;
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    Some(Request { method, path, headers, body, raw_head: buf[..head_end].to_vec() })
}

pub fn write_response<S: Write>(stream: &mut S, r: &Response) {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        r.status,
        r.reason,
        r.content_type,
        r.body.len()
    );
    for h in &r.extra_headers {
        head.push_str(h);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&r.body);
    let _ = stream.flush();
}
