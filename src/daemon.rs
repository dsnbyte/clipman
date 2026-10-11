//! Background process: clipboard capture, history storage, tray and IPC.
//! Deliberately free of any GUI toolkit so it stays tiny while idle.

use crate::clipboard::{self, ClipHandle, Limits};
use crate::config::Config;
use crate::model::*;
use crate::store::Store;
use crate::tray::{self, ClipTray};
use crate::{ipc, system};
use anyhow::Result;
use image::ImageFormat;
use ksni::blocking::Handle;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Quiet,
    Show,
    Settings,
}

struct Daemon {
    store: Store,
    cfg: Config,
    clip: ClipHandle,
    tray: Option<Handle<ClipTray>>,
    paused: bool,
    ui: Option<Child>,
    exe: PathBuf,
}

pub fn run(start: Start) -> Result<()> {
    let exe = std::env::current_exe()?;
    let (tx, rx) = mpsc::channel::<Msg>();

    if let Err(e) = ipc::listen(tx.clone()) {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            // Already running: just forward the request.
            match start {
                Start::Show => ipc::send("show")?,
                Start::Settings => ipc::send("settings")?,
                Start::Quiet => eprintln!("clipman is already running"),
            }
            return Ok(());
        }
        return Err(e.into());
    }

    let cfg = Config::load();
    let store = Store::open()?;
    let clip = clipboard::spawn(
        tx.clone(),
        Limits {
            max_text: cfg.max_text_kb * 1024,
            max_image: cfg.max_image_mb * 1024 * 1024,
        },
    )?;
    let tray = ClipTray::spawn(tx.clone());

    let mut d = Daemon {
        store,
        cfg,
        clip,
        tray,
        paused: false,
        ui: None,
        exe: exe.clone(),
    };
    d.refresh_tray();

    // Make sure the global shortcut exists (best effort, never blocks startup).
    std::thread::spawn(move || {
        if let Err(e) = system::ensure_shortcut(&exe) {
            log::warn!("global shortcut not registered: {e}");
        }
    });

    match start {
        Start::Quiet => {}
        Start::Show => d.open_ui(false),
        Start::Settings => d.open_ui(true),
    }

    for msg in rx {
        match msg {
            Msg::Clip(c) => d.on_capture(c),
            Msg::Cmd(Cmd::Quit) => break,
            Msg::Cmd(cmd) => d.on_cmd(cmd),
        }
    }
    d.kill_ui();
    ipc::cleanup();
    Ok(())
}

impl Daemon {
    fn on_capture(&mut self, c: Captured) {
        if self.paused {
            return;
        }
        let result = (|| -> Result<()> {
            if let Some(id) = self.store.find(c.hash) {
                self.store.touch(id)?;
            } else {
                let dims = match &c.payload {
                    Payload::Image(png) => self.save_image(png, c.hash)?,
                    _ => (0, 0),
                };
                self.store.insert(&c.payload, c.hash, dims)?;
                self.store.trim(self.cfg.max_items)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            log::warn!("failed to record clipboard item: {e}");
        }
        self.refresh_tray();
    }

    /// Writes the PNG and a small thumbnail; returns the image size.
    fn save_image(&self, png: &[u8], hash: u64) -> Result<(u32, u32)> {
        // PNG header: signature(8) + IHDR length/type(8) + width(4) + height(4)
        anyhow::ensure!(png.len() > 24, "truncated PNG");
        let w = u32::from_be_bytes(png[16..20].try_into()?);
        let h = u32::from_be_bytes(png[20..24].try_into()?);
        std::fs::write(self.store.image_path(hash), png)?;
        // Skip thumbnailing absurdly large images to keep memory use low.
        if (w as u64) * (h as u64) <= 36_000_000 {
            let img = image::load_from_memory_with_format(png, ImageFormat::Png)?;
            img.thumbnail(96, 96)
                .save_with_format(self.store.thumb_path(hash), ImageFormat::Png)?;
        }
        Ok((w, h))
    }

    fn on_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Toggle => {
                if self.ui_alive() {
                    self.kill_ui();
                } else {
                    self.open_ui(false);
                }
            }
            Cmd::Show => {
                if !self.ui_alive() {
                    self.open_ui(false);
                }
            }
            Cmd::Settings => {
                self.kill_ui();
                self.open_ui(true);
            }
            Cmd::Copy(id) => match self.store.payload(id) {
                Ok(Some(p)) => {
                    self.clip.set(p);
                    let _ = self.store.touch(id);
                    self.refresh_tray();
                }
                _ => log::warn!("item {id} not found"),
            },
            Cmd::Refresh => {
                self.cfg = Config::load();
                let _ = self.store.trim(self.cfg.max_items);
                self.refresh_tray();
            }
            Cmd::TogglePause => {
                self.paused = !self.paused;
                self.clip.set_paused(self.paused);
                self.refresh_tray();
            }
            Cmd::Quit => {}
        }
    }

    fn refresh_tray(&mut self) {
        let Some(handle) = &self.tray else { return };
        let recent: Vec<(i64, String)> = self
            .store
            .list(Filter::All, "", 8)
            .unwrap_or_default()
            .iter()
            .map(|it| (it.id, tray::menu_label(it)))
            .collect();
        let paused = self.paused;
        handle.update(|t| {
            t.recent = recent;
            t.paused = paused;
        });
    }

    // ---- UI process management -------------------------------------------

    fn ui_alive(&mut self) -> bool {
        match self.ui.as_mut().map(Child::try_wait) {
            Some(Ok(None)) => true,
            _ => {
                self.ui = None;
                false
            }
        }
    }

    fn open_ui(&mut self, settings: bool) {
        if self.ui_alive() {
            return;
        }
        let mut cmd = Command::new(&self.exe);
        cmd.arg("ui");
        if settings {
            cmd.arg("--settings");
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null());
        match cmd.spawn() {
            Ok(child) => self.ui = Some(child),
            Err(e) => log::error!("cannot start UI: {e}"),
        }
    }

    fn kill_ui(&mut self) {
        if let Some(mut c) = self.ui.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
