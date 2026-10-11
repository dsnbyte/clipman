//! Ubuntu/GNOME integration: global shortcut (custom keybinding) and autostart.

use crate::config;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

pub const DEFAULT_SHORTCUT: &str = "<Control><Alt>v";

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const LIST_KEY: &str = "custom-keybindings";
const KB_PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipman/";

fn gsettings(args: &[&str]) -> Result<String> {
    let out = Command::new("gsettings")
        .args(args)
        .output()
        .context("gsettings is not available")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn kb_schema() -> String {
    format!("{MEDIA_KEYS}.custom-keybinding:{KB_PATH}")
}

fn registered_paths() -> Result<Vec<String>> {
    let raw = gsettings(&["get", MEDIA_KEYS, LIST_KEY])?;
    Ok(raw
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect())
}

/// Current accelerator (GTK syntax, e.g. `<Control><Alt>v`) if our entry exists.
pub fn current_shortcut() -> Option<String> {
    if !registered_paths().ok()?.iter().any(|p| p == KB_PATH) {
        return None;
    }
    let v = gsettings(&["get", &kb_schema(), "binding"]).ok()?;
    let v = v.trim_matches('\'').to_string();
    (!v.is_empty()).then_some(v)
}

fn toggle_command(exe: &Path) -> String {
    format!("{} toggle", exe.display())
}

/// Registers the shortcut entry if missing and keeps its command path current.
/// Never overrides a binding the user changed.
pub fn ensure_shortcut(exe: &Path) -> Result<()> {
    if registered_paths()?.iter().any(|p| p == KB_PATH) {
        let schema = kb_schema();
        let cmd = toggle_command(exe);
        if gsettings(&["get", &schema, "command"])?.trim_matches('\'') != cmd {
            gsettings(&["set", &schema, "command", &quote(&cmd)])?;
        }
        return Ok(());
    }
    set_shortcut(exe, DEFAULT_SHORTCUT)
}

pub fn set_shortcut(exe: &Path, accel: &str) -> Result<()> {
    let mut paths = registered_paths()?;
    if !paths.iter().any(|p| p == KB_PATH) {
        paths.push(KB_PATH.to_string());
        let list = format!(
            "[{}]",
            paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(", ")
        );
        gsettings(&["set", MEDIA_KEYS, LIST_KEY, &list])?;
    }
    let schema = kb_schema();
    gsettings(&["set", &schema, "name", "'Clipman'"])?;
    gsettings(&["set", &schema, "command", &quote(&toggle_command(exe))])?;
    gsettings(&["set", &schema, "binding", &quote(accel)])?;
    Ok(())
}

/// Converts a GPUI keystroke into a GTK accelerator string.
pub fn to_accel(key: &str, ctrl: bool, alt: bool, shift: bool, sup: bool) -> Option<String> {
    let name = match key {
        k if k.chars().count() == 1 => match k.chars().next()? {
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
            '-' => "minus".into(),
            '=' => "equal".into(),
            ',' => "comma".into(),
            '.' => "period".into(),
            '/' => "slash".into(),
            ';' => "semicolon".into(),
            '\'' => "apostrophe".into(),
            '[' => "bracketleft".into(),
            ']' => "bracketright".into(),
            '\\' => "backslash".into(),
            '`' => "grave".into(),
            _ => return None,
        },
        "space" => "space".into(),
        "enter" => "Return".into(),
        "tab" => "Tab".into(),
        "backspace" => "BackSpace".into(),
        "delete" => "Delete".into(),
        "insert" => "Insert".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "Page_Up".into(),
        "pagedown" => "Page_Down".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        k if k.starts_with('f') && k[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
            k.to_uppercase()
        }
        _ => return None,
    };
    let mut s = String::new();
    if ctrl {
        s.push_str("<Control>");
    }
    if alt {
        s.push_str("<Alt>");
    }
    if shift {
        s.push_str("<Shift>");
    }
    if sup {
        s.push_str("<Super>");
    }
    s.push_str(&name);
    Some(s)
}

/// Human readable form of an accelerator: `<Control><Alt>v` → `Ctrl + Alt + V`.
pub fn display_accel(accel: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = accel;
    while let Some(stripped) = rest.strip_prefix('<') {
        let Some(end) = stripped.find('>') else { break };
        parts.push(match &stripped[..end] {
            "Control" | "Primary" => "Ctrl".to_string(),
            other => other.to_string(),
        });
        rest = &stripped[end + 1..];
    }
    let key = match rest {
        "minus" => "-".to_string(),
        "equal" => "=".to_string(),
        "comma" => ",".to_string(),
        "period" => ".".to_string(),
        "slash" => "/".to_string(),
        "semicolon" => ";".to_string(),
        "apostrophe" => "'".to_string(),
        "bracketleft" => "[".to_string(),
        "bracketright" => "]".to_string(),
        "backslash" => "\\".to_string(),
        "grave" => "`".to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    };
    parts.push(key);
    parts.join(" + ")
}

// ---- autostart ----------------------------------------------------------

fn autostart_file() -> std::path::PathBuf {
    config::config_dir().join("autostart").join("clipman.desktop")
}

pub fn autostart_enabled() -> bool {
    autostart_file().exists()
}

pub fn desktop_entry(exe: &Path, autostart: bool) -> String {
    let mut s = format!(
        "[Desktop Entry]\nType=Application\nName=Clipman\nComment=Lightweight clipboard manager\n\
         Exec={}\nIcon=edit-paste\nTerminal=false\nCategories=Utility;\nStartupWMClass=clipman\n",
        exe.display()
    );
    if autostart {
        s.push_str("X-GNOME-Autostart-enabled=true\nNoDisplay=true\n");
    }
    s
}

pub fn set_autostart(exe: &Path, enabled: bool) -> Result<()> {
    let path = autostart_file();
    if enabled {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, desktop_entry(exe, true))?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn remove_shortcut() -> Result<()> {
    let paths = registered_paths()?;
    if paths.iter().any(|p| p == KB_PATH) {
        let rest: Vec<String> = paths.into_iter().filter(|p| p != KB_PATH).collect();
        let list = if rest.is_empty() {
            "@as []".to_string()
        } else {
            format!("[{}]", rest.iter().map(|p| quote(p)).collect::<Vec<_>>().join(", "))
        };
        gsettings(&["set", MEDIA_KEYS, LIST_KEY, &list])?;
        for key in ["name", "command", "binding"] {
            let _ = gsettings(&["reset", &kb_schema(), key]);
        }
    }
    Ok(())
}

fn applications_file() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("applications").join("clipman.desktop")
}

/// Installs the launcher entry, autostart entry and default shortcut.
pub fn install(exe: &Path) -> Result<()> {
    let app = applications_file();
    std::fs::create_dir_all(app.parent().unwrap())?;
    std::fs::write(&app, desktop_entry(exe, false))?;
    set_autostart(exe, true)?;
    if let Err(e) = ensure_shortcut(exe) {
        eprintln!("warning: could not register shortcut: {e}");
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let _ = std::fs::remove_file(applications_file());
    let _ = std::fs::remove_file(autostart_file());
    remove_shortcut()
}
