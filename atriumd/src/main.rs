//! atriumd — the Peios web session host.
//!
//! Slice 1: a TCP listener on 0.0.0.0:8080 answering every HTTP request with
//! one page. No dependencies: the HTTP handling is the minimum needed to make
//! a browser render a body, and it is replaced wholesale when the daemon
//! grows a real server. What is meant to last from this slice is the shape —
//! a peinit service, Notify readiness, kmsg-mirrored logging, and packaging.

mod log;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixDatagram;
use std::process::ExitCode;
use std::time::Duration;

const LISTEN_ADDR: &str = "0.0.0.0:8080";

const PAGE: &str = "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Atrium</title></head>\n<body><h1>Hello World</h1></body></html>\n";

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

/// Consume the request head (up to the blank line, or 8 KiB, or a second of
/// silence) and answer with the page. Whatever the request was.
fn serve(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                head.extend_from_slice(&buf[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() >= 8192 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        PAGE.len(),
        PAGE
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn main() -> ExitCode {
    let listener = match TcpListener::bind(LISTEN_ADDR) {
        Ok(l) => l,
        Err(e) => {
            log::error(format_args!("listen on {LISTEN_ADDR}: {e}"));
            return ExitCode::FAILURE;
        }
    };
    log::info(format_args!("listening on http://{LISTEN_ADDR}"));
    notify_ready();
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => serve(stream),
            Err(e) => log::warn(format_args!("accept: {e}")),
        }
    }
    ExitCode::SUCCESS
}
