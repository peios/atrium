//! Lines on stderr, mirrored to the kernel log. Same shape as atriumd's.
//!
//! The server runs privilege-stripped; opening `/dev/kmsg` for write may be
//! refused, in which case only stderr (captured by peinit) carries the line.

use std::fmt::Arguments;
use std::io::Write;
use std::sync::OnceLock;

fn kmsg() -> Option<&'static std::fs::File> {
    static KMSG: OnceLock<Option<std::fs::File>> = OnceLock::new();
    KMSG.get_or_init(|| std::fs::OpenOptions::new().write(true).open("/dev/kmsg").ok()).as_ref()
}

fn emit(level: &str, args: Arguments<'_>) {
    let line = format!("atrium-server: {level}: {args}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
    if let Some(mut k) = kmsg() {
        let priority = match level {
            "error" => 3,
            "warn" => 4,
            _ => 6,
        };
        let _ = k.write_all(format!("<{priority}>{line}").as_bytes());
    }
}

pub fn info(args: Arguments<'_>) {
    emit("info", args);
}

pub fn warn(args: Arguments<'_>) {
    emit("warn", args);
}

pub fn error(args: Arguments<'_>) {
    emit("error", args);
}
