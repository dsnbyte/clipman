mod clipboard;
mod config;
mod daemon;
mod ipc;
mod model;
mod store;
mod system;
mod tray;
mod ui;

use daemon::Start;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

const HELP: &str = "\
Clipman — lightweight clipboard manager for Ubuntu

USAGE:
    clipman                Start the background service (or open the window if already running)
    clipman toggle         Show/hide the clipboard window (bind this to a shortcut)
    clipman show           Show the clipboard window
    clipman settings       Open the settings
    clipman pause          Pause/resume capturing
    clipman quit           Stop the background service
    clipman install        Add launcher, autostart entry and the default shortcut
    clipman uninstall      Remove launcher, autostart entry and shortcut

Set CLIPMAN_LOG=debug for verbose logs.";

/// Sends a command to the running service, starting it first if necessary.
fn client(cmd: &str, start: Start) -> anyhow::Result<()> {
    if ipc::send(cmd).is_ok() {
        return Ok(());
    }
    if cmd == "quit" {
        return Ok(());
    }
    // Service not running: start it detached, asking it to show the window.
    let exe = std::env::current_exe()?;
    let mut c = Command::new(exe);
    c.arg("daemon");
    match start {
        Start::Show => {
            c.arg("--show");
        }
        Start::Settings => {
            c.arg("--settings");
        }
        Start::Quiet => {}
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::new().filter_or("CLIPMAN_LOG", "warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);

    match args.first().map(String::as_str) {
        None => {
            if ipc::is_running() {
                ipc::send("show")?;
                Ok(())
            } else {
                daemon::run(Start::Quiet)
            }
        }
        Some("daemon") => daemon::run(if flag("--show") {
            Start::Show
        } else if flag("--settings") {
            Start::Settings
        } else {
            Start::Quiet
        }),
        Some("ui") => {
            ui::run(flag("--settings"));
            Ok(())
        }
        Some("toggle") => client("toggle", Start::Show),
        Some("show") => client("show", Start::Show),
        Some("settings") => client("settings", Start::Settings),
        Some("pause") => client("pause", Start::Quiet),
        Some("quit") => client("quit", Start::Quiet),
        Some("install") => {
            system::install(&std::env::current_exe()?)?;
            println!("Installed launcher, autostart entry and shortcut.");
            Ok(())
        }
        Some("uninstall") => {
            system::uninstall()?;
            println!("Removed launcher, autostart entry and shortcut.");
            Ok(())
        }
        Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("clipman {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
