//! StatusNotifierItem tray icon (shown in the Ubuntu top bar via AppIndicator).

use crate::model::*;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Category, MenuItem, Tray};
use std::sync::mpsc::Sender;

pub struct ClipTray {
    tx: Sender<Msg>,
    pub paused: bool,
}

impl ClipTray {
    pub fn spawn(tx: Sender<Msg>) -> Option<Handle<ClipTray>> {
        let tray = ClipTray {
            tx,
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

impl Tray for ClipTray {
    const MENU_ON_ACTIVATE: bool = true;

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
        vec![
            StandardItem {
                label: "Open Clipman".into(),
                icon_name: "edit-paste-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Cmd::Show)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Pause capturing".into(),
                checked: self.paused,
                activate: Box::new(|t: &mut Self| t.send(Cmd::TogglePause)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings…".into(),
                icon_name: "preferences-system-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Cmd::Settings)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(Cmd::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
