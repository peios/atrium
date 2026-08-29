//! The app catalogue: what is installed, read from the filesystem.
//!
//! An app is a directory `/usr/share/atrium/apps/<id>/` (reverse-DNS id) holding a
//! `manifest.toml` and whatever static files its entry needs. Every
//! launchable thing is one app — no nesting; grouping in the launcher is a
//! `category` string, not structure. Packages ship these directories;
//! nothing runs at install and nothing registers: presence is the catalogue.
//!
//! Read on every request rather than cached — the catalogue changes when
//! packages are installed, that is rare, and a directory listing is cheap.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `ATRIUM_APPS_DIR` overrides it, for a checkout.
const APPS_DIR: &str = "/usr/share/atrium/apps";

#[derive(Debug, Deserialize)]
struct ManifestFile {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    color: Option<String>,
    /// Name of a glyph from the shell's built-in stroked icon set, used
    /// when the app ships no icon file.
    #[serde(default)]
    glyph: Option<String>,
    #[serde(default = "default_entry")]
    entry: String,
}

fn default_entry() -> String {
    "index.html".into()
}

/// What the shell sees.
#[derive(Debug, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    /// URL of the icon, served by us, or null.
    pub icon: Option<String>,
    pub color: Option<String>,
    pub glyph: Option<String>,
    /// URL of the entry document.
    pub entry: String,
}

pub fn dir() -> PathBuf {
    std::env::var_os("ATRIUM_APPS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(APPS_DIR))
}

/// A directory name that is safe to use as an id and in a URL. Ids are
/// reverse-DNS (`org.peios.about`); the directory is the id.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !id.starts_with(['-', '.'])
        && !id.ends_with('.')
        && !id.contains("..")
}

pub fn catalogue() -> Vec<App> {
    let mut apps = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir()) else { return apps };
    for entry in entries.flatten() {
        let dir_name = entry.file_name();
        let Some(dir_id) = dir_name.to_str() else { continue };
        if !valid_id(dir_id) {
            continue;
        }
        let path = entry.path().join("manifest.toml");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let m: ManifestFile = match toml::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("atrium-session: {}: {e}", path.display());
                continue;
            }
        };
        // The directory is the id; a manifest that disagrees is a broken
        // package, not a second opinion.
        if m.id != dir_id {
            eprintln!("atrium-session: {}: id {:?} does not match its directory", path.display(), m.id);
            continue;
        }
        let file_url = |f: &str| format!("/apps/{dir_id}/{}", f.trim_start_matches('/'));
        apps.push(App {
            id: m.id,
            name: m.name,
            description: m.description,
            category: m.category,
            icon: m.icon.as_deref().map(file_url),
            color: m.color,
            glyph: m.glyph,
            entry: file_url(&m.entry),
        });
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// Resolve `/apps/<id>/<file>` to a file inside that app's directory, or
/// nothing. Rejects anything that could leave the directory.
pub fn resolve(url_path: &str) -> Option<PathBuf> {
    let rest = url_path.strip_prefix("/apps/")?;
    let (id, file) = rest.split_once('/')?;
    if !valid_id(id) || file.is_empty() {
        return None;
    }
    let mut out = dir().join(id);
    for seg in file.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') {
            return None;
        }
        out.push(seg);
    }
    if out.is_file() { Some(out) } else { None }
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
