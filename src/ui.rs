//! The GPUI window: history browser + settings. Runs as a short-lived process
//! started by the daemon, so it costs nothing while the window is closed.

use crate::config::Config;
use crate::model::*;
use crate::store::Store;
use crate::{ipc, system};
use gpui::{
    App, Application, AssetSource, Bounds, ClickEvent, Context, FocusHandle, FontWeight,
    KeyBinding, KeyDownEvent, ObjectFit, Render, Rgba, ScrollStrategy, SharedString, StyledImage,
    TitlebarOptions, UniformListScrollHandle, Window, WindowAppearance, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowKind, WindowOptions, actions, div, img, prelude::*, px,
    rgb, rgba, size, svg, uniform_list,
};
use std::borrow::Cow;
use std::ops::Range;
use std::path::PathBuf;

actions!(
    clipman,
    [
        SelectNext,
        SelectPrev,
        PageDown,
        PageUp,
        Confirm,
        DeleteItem,
        TogglePin,
        Close,
        NextFilter,
        PrevFilter,
        OpenSettings,
    ]
);

const ROW_H: f32 = 62.0;
const LIST_LIMIT: usize = 500;
const SIZES: [usize; 7] = [25, 50, 100, 200, 500, 1000, 2000];

// ---- assets ---------------------------------------------------------------

struct Assets;

macro_rules! icons {
    ($($name:literal),*) => {
        fn icon_bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $name, ".svg") =>
                    Some(include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice()),)*
                _ => None,
            }
        }
    };
}
icons!("x", "trash", "search", "pin", "pin-filled", "text", "image", "files", "settings", "back", "forward", "check");

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

// ---- theme ----------------------------------------------------------------

#[derive(Clone, Copy)]
struct Theme {
    bg: Rgba,
    panel: Rgba,
    border: Rgba,
    text: Rgba,
    muted: Rgba,
    accent: Rgba,
    on_accent: Rgba,
    selected: Rgba,
    hover: Rgba,
    chip: Rgba,
    danger: Rgba,
}

fn is_dark(appearance: WindowAppearance) -> bool {
    if matches!(appearance, WindowAppearance::Dark | WindowAppearance::VibrantDark) {
        return true;
    }
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("prefer-dark"))
        .unwrap_or(false)
}

fn theme(appearance: WindowAppearance) -> Theme {
    if is_dark(appearance) {
        Theme {
            bg: rgb(0x262626),
            panel: rgb(0x333333),
            border: rgb(0x474747),
            text: rgb(0xf0f0f0),
            muted: rgb(0x9b9b9b),
            accent: rgb(0xe95420),
            on_accent: rgb(0xffffff),
            selected: rgba(0xe9542044),
            hover: rgba(0xffffff12),
            chip: rgb(0x383838),
            danger: rgb(0xff6b5e),
        }
    } else {
        Theme {
            bg: rgb(0xfafafa),
            panel: rgb(0xeeeeee),
            border: rgb(0xd4d4d4),
            text: rgb(0x202020),
            muted: rgb(0x6e6e6e),
            accent: rgb(0xe95420),
            on_accent: rgb(0xffffff),
            selected: rgba(0xe9542030),
            hover: rgba(0x00000010),
            chip: rgb(0xe4e4e4),
            danger: rgb(0xc7162b),
        }
    }
}

// ---- view -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    List,
    Settings,
}

struct Root {
    store: Store,
    cfg: Config,
    exe: PathBuf,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    screen: Screen,
    query: String,
    filter: Filter,
    items: Vec<ItemMeta>,
    selected: usize,
    was_active: bool,
    recording: bool,
    confirm_clear: bool,
    notice: Option<String>,
    shortcut: Option<String>,
    autostart: bool,
}

impl Root {
    fn new(settings: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = Store::open().expect("cannot open history database");
        let focus = cx.focus_handle();
        focus.focus(window);

        // Behave like a popup: close as soon as the window loses focus.
        cx.observe_window_activation(window, |this, window, _cx| {
            if window.is_window_active() {
                this.was_active = true;
            } else if this.was_active && !this.recording {
                window.remove_window();
            }
        })
        .detach();

        let mut root = Root {
            store,
            cfg: Config::load(),
            exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("clipman")),
            focus,
            scroll: UniformListScrollHandle::new(),
            screen: if settings { Screen::Settings } else { Screen::List },
            query: String::new(),
            filter: Filter::All,
            items: Vec::new(),
            selected: 0,
            was_active: false,
            recording: false,
            confirm_clear: false,
            notice: None,
            shortcut: None,
            autostart: false,
        };
        root.items = root.store.list(root.filter, "", LIST_LIMIT).unwrap_or_default();
        root.load_system_state();
        root
    }

    fn load_system_state(&mut self) {
        self.shortcut = system::current_shortcut();
        self.autostart = system::autostart_enabled();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.items = self
            .store
            .list(self.filter, &self.query, LIST_LIMIT)
            .unwrap_or_default();
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        cx.notify();
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        self.selected = ix.min(self.items.len() - 1);
        self.scroll.scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let next = (self.selected as isize + delta).clamp(0, self.items.len() as isize - 1);
        self.select(next.max(0) as usize, cx);
    }

    fn set_filter(&mut self, f: Filter, cx: &mut Context<Self>) {
        self.filter = f;
        self.selected = 0;
        self.reload(cx);
    }

    fn cycle_filter(&mut self, dir: isize, cx: &mut Context<Self>) {
        let i = Filter::ALL.iter().position(|f| *f == self.filter).unwrap_or(0) as isize;
        let n = Filter::ALL.len() as isize;
        self.set_filter(Filter::ALL[((i + dir + n) % n) as usize], cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(self.selected) else {
            return;
        };
        match ipc::send(&format!("copy {}", item.id)) {
            Ok(()) => window.remove_window(),
            Err(_) => {
                self.notice = Some("Clipman background service is not running".into());
                cx.notify();
            }
        }
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(it) = self.items.get(self.selected) {
            let _ = self.store.delete(it.id);
            let _ = ipc::send("refresh");
            self.reload(cx);
        }
    }

    fn toggle_pin_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(it) = self.items.get(ix) else { return };
        let id = it.id;
        let _ = self.store.toggle_pin(id);
        let _ = ipc::send("refresh");
        self.reload(cx);
        if let Some(pos) = self.items.iter().position(|i| i.id == id) {
            self.select(pos, cx);
        }
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Settings;
        self.notice = None;
        self.confirm_clear = false;
        self.load_system_state();
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::List;
        self.recording = false;
        self.notice = None;
        self.confirm_clear = false;
        self.reload(cx);
    }

    fn change_max(&mut self, dir: isize, cx: &mut Context<Self>) {
        let i = SIZES
            .iter()
            .position(|s| *s >= self.cfg.max_items)
            .unwrap_or(SIZES.len() - 1) as isize;
        let j = (i + dir).clamp(0, SIZES.len() as isize - 1) as usize;
        self.cfg.max_items = SIZES[j];
        if let Err(e) = self.cfg.save() {
            self.notice = Some(format!("Cannot save settings: {e}"));
        }
        let _ = ipc::send("refresh");
        cx.notify();
    }

    fn toggle_autostart(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = system::set_autostart(&self.exe, !self.autostart) {
            self.notice = Some(format!("Cannot change autostart: {e}"));
        }
        self.autostart = system::autostart_enabled();
        cx.notify();
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        if self.confirm_clear {
            let _ = self.store.clear_unpinned();
            let _ = ipc::send("refresh");
            self.confirm_clear = false;
            self.notice = Some("History cleared (pinned items kept)".into());
        } else {
            self.confirm_clear = true;
        }
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;

        if self.recording {
            if k.key == "escape" {
                self.recording = false;
                self.notice = None;
            } else if !matches!(k.key.as_str(), "shift" | "control" | "alt" | "platform" | "function") {
                if !(m.control || m.alt || m.platform) {
                    self.notice = Some("Include Ctrl, Alt or Super in the shortcut".into());
                } else if let Some(accel) =
                    system::to_accel(&k.key, m.control, m.alt, m.shift, m.platform)
                {
                    match system::set_shortcut(&self.exe, &accel) {
                        Ok(()) => {
                            self.shortcut = Some(accel);
                            self.notice = None;
                        }
                        Err(e) => self.notice = Some(format!("Cannot set shortcut: {e}")),
                    }
                    self.recording = false;
                } else {
                    self.notice = Some("That key cannot be used".into());
                }
            }
            cx.notify();
            return;
        }

        if self.screen != Screen::List {
            return;
        }
        let mut changed = false;
        if k.key == "backspace" && !m.control && !m.alt {
            changed = self.query.pop().is_some();
        } else if m.control && !m.alt && k.key == "u" {
            changed = !self.query.is_empty();
            self.query.clear();
        } else if m.control && !m.alt && k.key == "v" {
            if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
                let line: String = t.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).collect();
                self.query.push_str(&line);
                changed = true;
            }
        } else if !m.control && !m.alt && !m.platform {
            if let Some(ch) = &k.key_char {
                if !ch.is_empty() && ch.chars().all(|c| !c.is_control()) {
                    self.query.push_str(ch);
                    changed = true;
                }
            }
        }
        if changed {
            self.selected = 0;
            self.reload(cx);
        }
    }
}

// ---- small widgets --------------------------------------------------------

fn icon(path: &'static str, px_size: f32, color: Rgba) -> impl IntoElement {
    svg().path(path).size(px(px_size)).flex_none().text_color(color)
}

fn button(id: &'static str, label: impl Into<SharedString>, t: Theme, primary: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px_3()
        .rounded(px(7.))
        .cursor_pointer()
        .text_size(px(13.))
        .when(primary, |d| d.bg(t.accent).text_color(t.on_accent))
        .when(!primary, |d| {
            d.bg(t.chip).text_color(t.text).hover(|s| s.bg(t.border))
        })
        .child(label.into())
}

fn icon_button(id: impl Into<gpui::ElementId>, path: &'static str, color: Rgba, t: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(t.hover))
        .child(icon(path, 15., color))
}

impl Root {
    fn search_bar(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let caret = div().w(px(1.5)).h(px(18.)).bg(t.accent).flex_none();
        let field = if self.query.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .child(caret)
                .child(
                    div()
                        .ml_1()
                        .text_color(t.muted)
                        .child("Type to search clipboard history"),
                )
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .overflow_hidden()
                .child(div().whitespace_nowrap().child(self.query.clone()))
                .child(caret)
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(42.))
            .mx_3()
            .mt_3()
            .px_3()
            .rounded(px(9.))
            .bg(t.panel)
            .child(icon("icons/search.svg", 16., t.muted))
            .child(field)
            .child(
                icon_button("open-settings", "icons/settings.svg", t.muted, t).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(cx)),
                ),
            )
    }

    fn filter_bar(&self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .children(Filter::ALL.iter().map(|&f| {
                let active = f == self.filter;
                div()
                    .id(f.label())
                    .px_3()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .when(active, |d| d.bg(t.accent).text_color(t.on_accent))
                    .when(!active, |d| {
                        d.bg(t.chip).text_color(t.muted).hover(|s| s.bg(t.border))
                    })
                    .child(f.label())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_filter(f, cx)))
            }))
    }

    fn row(&self, ix: usize, it: &ItemMeta, t: Theme, now: i64, cx: &Context<Self>) -> gpui::AnyElement {
        let selected = ix == self.selected;
        let thumb = (it.kind == Kind::Image)
            .then(|| self.store.thumb_path(it.hash))
            .filter(|p| p.exists());

        let lead = match thumb {
            Some(path) => div()
                .size(px(42.))
                .flex_none()
                .rounded(px(6.))
                .overflow_hidden()
                .bg(t.chip)
                .child(img(path).size_full().object_fit(ObjectFit::Cover))
                .into_any_element(),
            None => div()
                .size(px(42.))
                .flex_none()
                .rounded(px(6.))
                .bg(t.chip)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(
                    match it.kind {
                        Kind::Text => "icons/text.svg",
                        Kind::Image => "icons/image.svg",
                        Kind::Files => "icons/files.svg",
                    },
                    20.,
                    t.muted,
                ))
                .into_any_element(),
        };

        let meta = match it.kind {
            Kind::Text => format!("Text · {} · {}", human_size(it.size), ago(now, it.used_at)),
            Kind::Image => format!(
                "Image · {}×{} · {} · {}",
                it.width,
                it.height,
                human_size(it.size),
                ago(now, it.used_at)
            ),
            Kind::Files => format!(
                "{} · {}",
                if it.size == 1 { "1 file".to_string() } else { format!("{} files", it.size) },
                ago(now, it.used_at)
            ),
        };
        let preview = if it.kind == Kind::Image { "Image".to_string() } else { it.preview.clone() };
        let pinned = it.pinned;

        div()
            .id(("row", ix))
            .group("row")
            .h(px(ROW_H))
            .mx_2()
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(9.))
            .cursor_pointer()
            .when(selected, |d| d.bg(t.selected))
            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.selected = ix;
                this.confirm(window, cx);
            }))
            .child(lead)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(17.))
                            .max_h(px(34.))
                            .overflow_hidden()
                            .line_clamp(2)
                            .child(preview),
                    )
                    .child(div().text_size(px(11.)).text_color(t.muted).child(meta)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_none()
                    .when(pinned, |d| d.child(icon("icons/pin-filled.svg", 13., t.accent)))
                    .child(
                        div()
                            .flex()
                            .opacity(if selected { 1.0 } else { 0.0 })
                            .group_hover("row", |s| s.opacity(1.0))
                            .child(
                                icon_button(("pin", ix), "icons/pin.svg", t.muted, t).on_click(
                                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.toggle_pin_at(ix, cx);
                                    }),
                                ),
                            )
                            .child(
                                icon_button(("del", ix), "icons/trash.svg", t.danger, t).on_click(
                                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.selected = ix;
                                        this.delete_selected(cx);
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn list_view(&mut self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let n = self.items.len();
        let body = if n == 0 {
            let msg = if !self.query.is_empty() {
                "No matching items"
            } else if self.filter == Filter::All {
                "Nothing copied yet — copy something and it will show up here"
            } else {
                "No items in this category"
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.muted)
                .child(msg)
                .into_any_element()
        } else {
            uniform_list(
                "items",
                n,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    let now = now_secs();
                    range
                        .map(|ix| {
                            let it = this.items[ix].clone();
                            this.row(ix, &it, t, now, cx)
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .size_full()
            .into_any_element()
        };

        let hint = |key: &'static str, what: &'static str| {
            div()
                .flex()
                .gap_1()
                .child(div().text_color(t.text).child(key))
                .child(div().child(what))
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.search_bar(t, cx))
            .child(self.filter_bar(t, cx))
            .child(div().flex_1().min_h(px(0.)).child(body))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(30.))
                    .px_4()
                    .border_t_1()
                    .border_color(t.border)
                    .text_size(px(11.))
                    .text_color(t.muted)
                    .child(match &self.notice {
                        Some(n) => div().text_color(t.danger).child(n.clone()).into_any_element(),
                        None => div()
                            .flex()
                            .gap_3()
                            .child(hint("Enter", "copy"))
                            .child(hint("Ctrl+P", "pin"))
                            .child(hint("Del", "remove"))
                            .child(hint("Tab", "filter"))
                            .child(hint("Esc", "close"))
                            .into_any_element(),
                    })
                    .child(format!("{} item{}", n, if n == 1 { "" } else { "s" })),
            )
    }

    fn settings_view(&mut self, t: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, desc: &'static str, control: gpui::AnyElement| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .py_3()
                .border_b_1()
                .border_color(t.border)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().font_weight(FontWeight::MEDIUM).child(label))
                        .child(div().text_size(px(12.)).text_color(t.muted).child(desc)),
                )
                .child(control)
        };

        // Shortcut control
        let shortcut_text = if self.recording {
            "Press new shortcut…".to_string()
        } else {
            self.shortcut
                .as_deref()
                .map(system::display_accel)
                .unwrap_or_else(|| "Not set".into())
        };
        let shortcut_ctl = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .px_3()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .rounded(px(7.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(if self.recording { t.accent } else { t.border })
                    .text_size(px(13.))
                    .child(shortcut_text),
            )
            .child(
                button("rec", if self.recording { "Cancel" } else { "Change" }, t, false).on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.recording = !this.recording;
                        this.notice = None;
                        this.focus.focus(window);
                        cx.notify();
                    }),
                ),
            )
            .into_any_element();

        // History size control
        let size_ctl = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                icon_button("max-dec", "icons/back.svg", t.text, t)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.change_max(-1, cx))),
            )
            .child(
                div()
                    .w(px(56.))
                    .flex()
                    .justify_center()
                    .child(format!("{}", self.cfg.max_items)),
            )
            .child(
                icon_button("max-inc", "icons/forward.svg", t.text, t)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.change_max(1, cx))),
            )
            .into_any_element();

        // Autostart switch
        let on = self.autostart;
        let switch = div()
            .id("autostart")
            .w(px(40.))
            .h(px(22.))
            .rounded(px(11.))
            .p(px(2.))
            .flex()
            .cursor_pointer()
            .bg(if on { t.accent } else { t.border })
            .when(on, |d| d.justify_end())
            .child(div().size(px(18.)).rounded(px(9.)).bg(rgb(0xffffff)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_autostart(cx)))
            .into_any_element();

        let clear_ctl = button(
            "clear",
            if self.confirm_clear { "Click again to confirm" } else { "Clear" },
            t,
            self.confirm_clear,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.clear_history(cx)))
        .into_any_element();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(52.))
                    .px_3()
                    .child(
                        icon_button("back", "icons/back.svg", t.text, t)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back(cx))),
                    )
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child("Settings")),
            )
            .child(
                div()
                    .flex_1()
                    .px_5()
                    .flex()
                    .flex_col()
                    .child(row("Global shortcut", "Opens or closes Clipman from anywhere", shortcut_ctl))
                    .child(row("History size", "Unpinned items to keep", size_ctl))
                    .child(row("Start at login", "Run in the background after you sign in", switch))
                    .child(row("Clear history", "Removes everything except pinned items", clear_ctl)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(30.))
                    .px_4()
                    .border_t_1()
                    .border_color(t.border)
                    .text_size(px(11.))
                    .text_color(t.muted)
                    .child(match &self.notice {
                        Some(n) => div().text_color(t.danger).child(n.clone()),
                        None => div().child("Esc to go back"),
                    }),
            )
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window.appearance());
        let ctx = if self.recording {
            "ClipmanRecording"
        } else if self.screen == Screen::Settings {
            "ClipmanSettings"
        } else {
            "ClipmanList"
        };
        let body = match self.screen {
            Screen::List => self.list_view(t, cx).into_any_element(),
            Screen::Settings => self.settings_view(t, cx).into_any_element(),
        };

        div()
            .id("root")
            .key_context(ctx)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.move_selection(7, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.move_selection(-7, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &DeleteItem, _, cx| this.delete_selected(cx)))
            .on_action(cx.listener(|this, _: &TogglePin, _, cx| {
                let ix = this.selected;
                this.toggle_pin_at(ix, cx)
            }))
            .on_action(cx.listener(|this, _: &NextFilter, _, cx| this.cycle_filter(1, cx)))
            .on_action(cx.listener(|this, _: &PrevFilter, _, cx| this.cycle_filter(-1, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.open_settings(cx)))
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                if this.screen == Screen::Settings {
                    this.back(cx);
                } else {
                    window.remove_window();
                }
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .size_full()
            .bg(t.bg)
            .border_1()
            .border_color(t.border)
            .rounded(px(14.))
            .overflow_hidden()
            .text_color(t.text)
            .text_size(px(14.))
            .child(body)
    }
}

/// Opens the window and runs until it is closed.
pub fn run(settings: bool) {
    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("down", SelectNext, Some("ClipmanList")),
            KeyBinding::new("up", SelectPrev, Some("ClipmanList")),
            KeyBinding::new("ctrl-n", SelectNext, Some("ClipmanList")),
            KeyBinding::new("ctrl-j", SelectNext, Some("ClipmanList")),
            KeyBinding::new("ctrl-k", SelectPrev, Some("ClipmanList")),
            KeyBinding::new("pagedown", PageDown, Some("ClipmanList")),
            KeyBinding::new("pageup", PageUp, Some("ClipmanList")),
            KeyBinding::new("enter", Confirm, Some("ClipmanList")),
            KeyBinding::new("delete", DeleteItem, Some("ClipmanList")),
            KeyBinding::new("ctrl-p", TogglePin, Some("ClipmanList")),
            KeyBinding::new("tab", NextFilter, Some("ClipmanList")),
            KeyBinding::new("shift-tab", PrevFilter, Some("ClipmanList")),
            KeyBinding::new("ctrl-,", OpenSettings, Some("ClipmanList")),
            KeyBinding::new("escape", Close, Some("ClipmanList")),
            KeyBinding::new("escape", Close, Some("ClipmanSettings")),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(620.), px(560.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Clipman".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                focus: true,
                show: true,
                kind: WindowKind::Normal,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("clipman".into()),
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            },
            |window, cx| {
                window.activate_window();
                cx.new(|cx| Root::new(settings, window, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("clipman: cannot open window: {e}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
