//! Tiny line-based control protocol over a per-user unix socket.

use crate::model::{Cmd, Msg};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("clipman.sock"),
        None => PathBuf::from(format!("/tmp/clipman-{}.sock", std::env::var("USER").unwrap_or_default())),
    }
}

pub fn is_running() -> bool {
    UnixStream::connect(socket_path()).is_ok()
}

/// Sends one command line to the daemon.
pub fn send(line: &str) -> io::Result<()> {
    let mut s = UnixStream::connect(socket_path())?;
    s.set_write_timeout(Some(Duration::from_secs(1)))?;
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")
}

fn parse(line: &str) -> Option<Cmd> {
    let mut it = line.split_whitespace();
    Some(match it.next()? {
        "toggle" => Cmd::Toggle,
        "show" => Cmd::Show,
        "settings" => Cmd::Settings,
        "quit" => Cmd::Quit,
        "refresh" => Cmd::Refresh,
        "pause" => Cmd::TogglePause,
        "copy" => Cmd::Copy(it.next()?.parse().ok()?),
        _ => return None,
    })
}

/// Binds the control socket and forwards commands to the daemon loop.
/// Fails with `AddrInUse` if another daemon is already listening.
pub fn listen(tx: Sender<Msg>) -> io::Result<()> {
    let path = socket_path();
    if is_running() {
        return Err(io::Error::from(io::ErrorKind::AddrInUse));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for conn in listener.incoming().flatten() {
            let _ = conn.set_read_timeout(Some(Duration::from_millis(500)));
            let mut line = String::new();
            if BufReader::new(conn.take(256)).read_line(&mut line).is_ok() {
                if let Some(cmd) = parse(&line) {
                    if tx.send(Msg::Cmd(cmd)).is_err() {
                        break;
                    }
                }
            }
        }
    })?;
    Ok(())
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}
