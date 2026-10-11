//! Shared data types and small helpers.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Image,
    Files,
}

impl Kind {
    pub fn to_i64(self) -> i64 {
        match self {
            Kind::Text => 0,
            Kind::Image => 1,
            Kind::Files => 2,
        }
    }

    pub fn from_i64(v: i64) -> Kind {
        match v {
            1 => Kind::Image,
            2 => Kind::Files,
            _ => Kind::Text,
        }
    }
}

/// Clipboard content, used both for captured and for restored items.
#[derive(Clone, Debug)]
pub enum Payload {
    Text(String),
    Files(Vec<PathBuf>),
    /// PNG-encoded image bytes.
    Image(Vec<u8>),
}

impl Payload {
    pub fn kind(&self) -> Kind {
        match self {
            Payload::Text(_) => Kind::Text,
            Payload::Files(_) => Kind::Files,
            Payload::Image(_) => Kind::Image,
        }
    }

    /// Stable 64-bit content hash (FNV-1a) used for de-duplication.
    pub fn hash(&self) -> u64 {
        let seed = 0xcbf2_9ce4_8422_2325 ^ (self.kind().to_i64() as u64);
        match self {
            Payload::Text(t) => fnv(seed, t.as_bytes()),
            Payload::Image(b) => fnv(seed, b),
            Payload::Files(f) => fnv(seed, join_paths(f).as_bytes()),
        }
    }
}

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn join_paths(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A payload as captured from the clipboard, with its precomputed hash.
pub struct Captured {
    pub payload: Payload,
    pub hash: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    All,
    Pinned,
    Text,
    Images,
    Files,
}

impl Filter {
    pub const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Pinned,
        Filter::Text,
        Filter::Images,
        Filter::Files,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Pinned => "Pinned",
            Filter::Text => "Text",
            Filter::Images => "Images",
            Filter::Files => "Files",
        }
    }
}

/// Lightweight row shown in lists (no full content).
#[derive(Clone, Debug)]
pub struct ItemMeta {
    pub id: i64,
    pub kind: Kind,
    pub preview: String,
    pub size: i64,
    pub width: i64,
    pub height: i64,
    pub pinned: bool,
    pub used_at: i64,
    pub hash: u64,
}

/// Messages handled by the daemon's main loop.
pub enum Msg {
    Clip(Captured),
    Cmd(Cmd),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Toggle,
    Show,
    Settings,
    Quit,
    Copy(i64),
    Refresh,
    TogglePause,
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn human_size(bytes: i64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    }
}

pub fn ago(now: i64, ts: i64) -> String {
    let d = (now - ts).max(0);
    match d {
        0..=9 => "just now".into(),
        10..=59 => format!("{d}s ago"),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}

/// Collapses text into a short multi-line preview for list rows.
pub fn text_preview(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim_end)
        .skip_while(|l| l.trim().is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("\n");
    joined.chars().take(300).collect()
}

pub fn files_preview(files: &[PathBuf]) -> String {
    let name = |p: &PathBuf| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string_lossy().into_owned())
    };
    match files {
        [] => String::new(),
        [one] => {
            let parent = one.parent().map(Path::to_string_lossy).unwrap_or_default();
            format!("{}\n{}", name(one), parent)
        }
        [first, rest @ ..] => {
            let mut names = vec![name(first)];
            names.extend(rest.iter().take(2).map(name));
            let more = files.len().saturating_sub(3);
            let mut s = names.join(", ");
            if more > 0 {
                s.push_str(&format!(" (+{more} more)"));
            }
            s
        }
    }
}

// ---- file:// URI helpers -------------------------------------------------

pub fn path_to_uri(p: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in p.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    // Skip optional "localhost" authority.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(v) = rest
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}
