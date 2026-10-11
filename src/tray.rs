//! StatusNotifierItem tray icon (shown in the Ubuntu top bar via AppIndicator).

use crate::model::*;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Category, MenuItem, Tray};
use std::sync::mpsc::Sender;

pub struct ClipTray {
    tx: Sender<Msg>,
    pub recent: Vec<(i64, String)>,
    pub paused: bool,
}

impl ClipTray {
    pub fn spawn(tx: Sender<Msg>) -> Option<Handle<ClipTray>> {
        let tray = ClipTray {
            tx,
            recent: Vec::new(),
            paused: false,
        };
        match tray.assume_sni_available(true).spawn() {
            Ok(h) => Some(h),
            Err(e) => {
                log::warn!("tray icon unavailable: {e}");
                None
            }
        }
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }
}

/// One-line label for a history item.
pub fn menu_label(it: &ItemMeta) -> String {
    let base = match it.kind {
        Kind::Image => format!("[Image] {}×{}", it.width, it.height),
        Kind::Files => {
            let first = it.preview.lines().next().unwrap_or("");
            format!("[File] {first}")
        }
        Kind::Text => it.preview.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    let mut s: String = base.chars().take(48).collect();
    if base.chars().count() > 48 {
        s.push('…');
    }
    if it.pinned {
        s = format!("★ {s}");
    }
    s.replace('_', "__")
}

impl Tray for ClipTray {
    fn id(&self) -> String {
        "clipman".into()
    }

    fn title(&self) -> String {
        "Clipman".into()
    }

    fn icon_name(&self) -> String {
        "edit-paste-symbolic".into()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut m: Vec<MenuItem<Self>> = Vec::new();
        if self.recent.is_empty() {
            m.push(
                StandardItem {
                    label: "Clipboard history is empty".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }
        for (id, label) in &self.recent {
            let id = *id;
            m.push(
                StandardItem {
                    label: label.clone(),
                    activate: Box::new(move |t: &mut Self| t.send(Cmd::Copy(id))),
                    ..Default::default()
                }
                .into(),
            );
        }
        m.push(MenuItem::Separator);
        m.push(
            StandardItem {
                label: "Open Clipman".into(),
                icon_name: "edit-paste-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Cmd::Show)),
                ..Default::default()
            }
            .into(),
        );
        m.push(
            CheckmarkItem {
                label: "Pause capturing".into(),
                checked: self.paused,
                activate: Box::new(|t: &mut Self| t.send(Cmd::TogglePause)),
                ..Default::default()
            }
            .into(),
        );
        m.push(
            StandardItem {
                label: "Settings…".into(),
                icon_name: "preferences-system-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Cmd::Settings)),
                ..Default::default()
            }
            .into(),
        );
        m.push(MenuItem::Separator);
        m.push(
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Cmd::Quit)),
                ..Default::default()
            }
            .into(),
        );
        m
    }
}
