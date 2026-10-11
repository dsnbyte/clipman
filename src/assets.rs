//! Embedded SVG assets for the GPUI user interface.

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;

macro_rules! icon {
    ($name:literal, $file:literal) => {
        ($name, include_bytes!(concat!("../assets/icons/", $file)) as &'static [u8])
    };
}

const ICONS: &[(&str, &[u8])] = &[
    icon!("icons/back.svg", "back.svg"),
    icon!("icons/check.svg", "check.svg"),
    icon!("icons/forward.svg", "forward.svg"),
    icon!("icons/files.svg", "files.svg"),
    icon!("icons/image.svg", "image.svg"),
    icon!("icons/pin.svg", "pin.svg"),
    icon!("icons/pin-filled.svg", "pin-filled.svg"),
    icon!("icons/search.svg", "search.svg"),
    icon!("icons/settings.svg", "settings.svg"),
    icon!("icons/text.svg", "text.svg"),
    icon!("icons/trash.svg", "trash.svg"),
    icon!("icons/x.svg", "x.svg"),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        for (name, bytes) in ICONS {
            if *name == path {
                return Ok(Some(Cow::Borrowed(*bytes)));
            }
        }
        // Fallback: check filesystem if it exists
        if let Ok(data) = std::fs::read(path) {
            return Ok(Some(Cow::Owned(data)));
        }
        Ok(None)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        let mut out = Vec::new();
        for (name, _) in ICONS {
            if name.starts_with(&prefix) {
                out.push(SharedString::from(*name));
            }
        }
        Ok(out)
    }
}
