//! Event-driven X11 clipboard watcher and selection owner.
//!
//! * Changes are detected through XFixes selection notifications (no polling).
//! * Understands text, images (`image/png`) and file lists (`text/uri-list`,
//!   `x-special/gnome-copied-files`), including INCR transfers for big data.
//! * Works on GNOME/Wayland too, through XWayland's clipboard bridge.

use crate::model::*;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{self, ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        UTF8_STRING,
        STRING,
        INCR,
        CLIPMAN_PROP,
        CLIPMAN_WAKE,
        TEXT_PLAIN: b"text/plain",
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        URI_LIST: b"text/uri-list",
        GNOME_FILES: b"x-special/gnome-copied-files",
        IMAGE_PNG: b"image/png",
        PASSWORD_HINT: b"x-kde-passwordManagerHint",
    }
}

const TIMEOUT: Duration = Duration::from_millis(2500);

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_text: usize,
    pub max_image: usize,
}

struct Shared {
    queue: Mutex<VecDeque<Payload>>,
    paused: AtomicBool,
}

/// Cheap, cloneable handle used to control the clipboard thread.
#[derive(Clone)]
pub struct ClipHandle {
    conn: Arc<RustConnection>,
    win: Window,
    wake: Atom,
    shared: Arc<Shared>,
}

impl ClipHandle {
    /// Makes `payload` the current clipboard content.
    pub fn set(&self, payload: Payload) {
        self.shared.queue.lock().unwrap().push_back(payload);
        let ev = ClientMessageEvent::new(32, self.win, self.wake, [0u32; 5]);
        let _ = self.conn.send_event(false, self.win, EventMask::NO_EVENT, ev);
        let _ = self.conn.flush();
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }
}

struct IncrSend {
    requestor: Window,
    property: Atom,
    ty: Atom,
    data: Vec<u8>,
    offset: usize,
}

struct Worker {
    conn: Arc<RustConnection>,
    win: Window,
    atoms: Atoms,
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    limits: Limits,
    owned: Option<Payload>,
    incr: Vec<IncrSend>,
    pending: VecDeque<Event>,
}

pub fn spawn(tx: Sender<Msg>, limits: Limits) -> Result<ClipHandle> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let conn = Arc::new(conn);
    let screen = &conn.setup().roots[screen_num];
    let win = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    let atoms = Atoms::new(&*conn)?.reply()?;
    xfixes::query_version(&*conn, 5, 0)?.reply()?;
    conn.xfixes_select_selection_input(win, atoms.CLIPBOARD, SelectionEventMask::SET_SELECTION_OWNER)?;
    conn.flush()?;

    let shared = Arc::new(Shared {
        queue: Mutex::new(VecDeque::new()),
        paused: AtomicBool::new(false),
    });
    let handle = ClipHandle {
        conn: conn.clone(),
        win,
        wake: atoms.CLIPMAN_WAKE,
        shared: shared.clone(),
    };
    let worker = Worker {
        conn,
        win,
        atoms,
        shared,
        tx,
        limits,
        owned: None,
        incr: Vec::new(),
        pending: VecDeque::new(),
    };
    std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || worker.run())?;
    Ok(handle)
}

impl Worker {
    fn run(mut self) {
        // Record whatever is on the clipboard right now.
        let owner = self
            .conn
            .get_selection_owner(self.atoms.CLIPBOARD)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.owner)
            .unwrap_or(NONE);
        if owner != NONE && owner != self.win {
            self.capture();
        }

        loop {
            let ev = match self.pending.pop_front() {
                Some(e) => e,
                None => match self.conn.wait_for_event() {
                    Ok(e) => e,
                    Err(err) => {
                        log::error!("X11 connection lost: {err}");
                        let _ = self.tx.send(Msg::Cmd(Cmd::Quit));
                        return;
                    }
                },
            };
            self.handle(ev);
        }
    }

    fn handle(&mut self, ev: Event) {
        let a = self.atoms;
        match ev {
            Event::XfixesSelectionNotify(e) if e.selection == a.CLIPBOARD => {
                if e.owner != self.win && e.owner != NONE {
                    self.capture();
                }
            }
            Event::SelectionRequest(e) => self.serve(e),
            Event::SelectionClear(e) if e.selection == a.CLIPBOARD => {
                self.owned = None;
                self.incr.clear();
            }
            Event::PropertyNotify(e) if e.state == Property::DELETE => {
                self.incr_next(e.window, e.atom)
            }
            Event::ClientMessage(e) if e.type_ == a.CLIPMAN_WAKE => self.drain_commands(),
            _ => {}
        }
    }

    fn drain_commands(&mut self) {
        let last = {
            let mut q = self.shared.queue.lock().unwrap();
            let last = q.pop_back();
            q.clear();
            last
        };
        if let Some(p) = last {
            self.owned = Some(p);
            self.incr.clear();
            let _ = self.conn.set_selection_owner(self.win, self.atoms.CLIPBOARD, CURRENT_TIME);
            let _ = self.conn.flush();
        }
    }

    // ---- reading ---------------------------------------------------------

    fn capture(&mut self) {
        if self.shared.paused.load(Ordering::Relaxed) {
            return;
        }
        let a = self.atoms;
        let Some((_, raw)) = self.fetch(a.TARGETS, 64 * 1024) else {
            return;
        };
        let targets: Vec<Atom> = raw
            .chunks_exact(4)
            .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
            .collect();
        if targets.contains(&a.PASSWORD_HINT) {
            return; // password managers mark secrets; never record them
        }
        let has = |t: Atom| targets.contains(&t);

        // 1. Files
        let mut payload = None;
        if has(a.GNOME_FILES) {
            payload = self
                .fetch(a.GNOME_FILES, 1 << 20)
                .and_then(|(_, d)| parse_uri_list(&d, true));
        }
        if payload.is_none() && has(a.URI_LIST) {
            payload = self
                .fetch(a.URI_LIST, 1 << 20)
                .and_then(|(_, d)| parse_uri_list(&d, false));
        }
        // 2. Text
        if payload.is_none() {
            for t in [a.UTF8_STRING, a.TEXT_PLAIN_UTF8, a.STRING, a.TEXT_PLAIN] {
                if !has(t) {
                    continue;
                }
                if let Some((_, d)) = self.fetch(t, self.limits.max_text) {
                    let text = if t == a.STRING {
                        d.iter().map(|&b| b as char).collect()
                    } else {
                        String::from_utf8_lossy(&d).into_owned()
                    };
                    if !text.trim().is_empty() {
                        payload = Some(Payload::Text(text));
                        break;
                    }
                }
            }
        }
        // 3. Image
        if payload.is_none() && has(a.IMAGE_PNG) {
            if let Some((_, d)) = self.fetch(a.IMAGE_PNG, self.limits.max_image) {
                if d.starts_with(b"\x89PNG") {
                    payload = Some(Payload::Image(d));
                }
            }
        }

        if let Some(payload) = payload {
            let hash = payload.hash();
            let _ = self.tx.send(Msg::Clip(Captured { payload, hash }));
        }
    }

    fn poll_event(&self, deadline: Instant) -> Option<Event> {
        loop {
            match self.conn.poll_for_event() {
                Ok(Some(e)) => return Some(e),
                Ok(None) => {}
                Err(_) => return None,
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Requests the selection converted to `target`, handling INCR transfers.
    fn fetch(&mut self, target: Atom, max: usize) -> Option<(Atom, Vec<u8>)> {
        let a = self.atoms;
        let c = self.conn.clone();
        c.delete_property(self.win, a.CLIPMAN_PROP).ok()?;
        c.convert_selection(self.win, a.CLIPBOARD, target, a.CLIPMAN_PROP, CURRENT_TIME)
            .ok()?;
        c.flush().ok()?;

        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.poll_event(deadline)? {
                Event::SelectionNotify(e) if e.requestor == self.win && e.target == target => {
                    if e.property == NONE {
                        return None;
                    }
                    break;
                }
                other => self.pending.push_back(other),
            }
        }

        let (ty, mut data) = self.read_prop(max)?;
        if ty != a.INCR {
            return Some((ty, data));
        }
        data.clear();
        loop {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                match self.poll_event(deadline)? {
                    Event::PropertyNotify(e)
                        if e.window == self.win
                            && e.atom == a.CLIPMAN_PROP
                            && e.state == Property::NEW_VALUE =>
                    {
                        break;
                    }
                    other => self.pending.push_back(other),
                }
            }
            let (_, chunk) = self.read_prop(max)?;
            if chunk.is_empty() {
                return Some((target, data));
            }
            data.extend_from_slice(&chunk);
            if data.len() > max {
                return None;
            }
        }
    }

    /// Reads (and deletes) our transfer property.
    fn read_prop(&self, max: usize) -> Option<(Atom, Vec<u8>)> {
        let c = &self.conn;
        let mut out = Vec::new();
        let mut offset = 0u32;
        let mut ty = 0;
        let mut exists = false;
        loop {
            let r = c
                .get_property(false, self.win, self.atoms.CLIPMAN_PROP, AtomEnum::ANY, offset, 1 << 18)
                .ok()?
                .reply()
                .ok()?;
            if r.format == 0 {
                break;
            }
            exists = true;
            ty = r.type_;
            offset += r.value.len().div_ceil(4) as u32;
            out.extend_from_slice(&r.value);
            if r.bytes_after == 0 || out.len() > max {
                break;
            }
        }
        let _ = c.delete_property(self.win, self.atoms.CLIPMAN_PROP);
        let _ = c.flush();
        (exists && out.len() <= max).then_some((ty, out))
    }

    // ---- serving ---------------------------------------------------------

    fn serve(&mut self, e: SelectionRequestEvent) {
        let prop = if e.property == NONE { e.target } else { e.property };
        let ok = self.try_serve(&e, prop).unwrap_or(false);
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: e.time,
            requestor: e.requestor,
            selection: e.selection,
            target: e.target,
            property: if ok { prop } else { NONE },
        };
        let _ = self.conn.send_event(false, e.requestor, EventMask::NO_EVENT, notify);
        let _ = self.conn.flush();
    }

    fn try_serve(&mut self, e: &SelectionRequestEvent, prop: Atom) -> Result<bool> {
        let a = self.atoms;
        let c = self.conn.clone();
        if e.selection != a.CLIPBOARD {
            return Ok(false);
        }
        let Some(owned) = &self.owned else {
            return Ok(false);
        };
        if e.target == a.TARGETS {
            let list = target_list(&a, owned);
            c.change_property32(PropMode::REPLACE, e.requestor, prop, AtomEnum::ATOM, &list)?;
            return Ok(true);
        }
        let Some(data) = encode(&a, owned, e.target) else {
            return Ok(false);
        };
        let limit = c.maximum_request_bytes().saturating_sub(256).min(256 * 1024);
        if data.len() <= limit {
            c.change_property8(PropMode::REPLACE, e.requestor, prop, e.target, &data)?;
            return Ok(true);
        }
        // Large payload: INCR protocol.
        c.change_window_attributes(
            e.requestor,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        c.change_property32(PropMode::REPLACE, e.requestor, prop, a.INCR, &[data.len() as u32])?;
        self.incr
            .retain(|t| !(t.requestor == e.requestor && t.property == prop));
        if self.incr.len() > 8 {
            self.incr.remove(0);
        }
        self.incr.push(IncrSend {
            requestor: e.requestor,
            property: prop,
            ty: e.target,
            data,
            offset: 0,
        });
        Ok(true)
    }

    fn incr_next(&mut self, window: Window, property: Atom) {
        let Some(pos) = self
            .incr
            .iter()
            .position(|t| t.requestor == window && t.property == property)
        else {
            return;
        };
        let chunk = self
            .conn
            .maximum_request_bytes()
            .saturating_sub(256)
            .min(256 * 1024);
        let t = &mut self.incr[pos];
        let end = (t.offset + chunk).min(t.data.len());
        let _ = self.conn.change_property8(
            PropMode::REPLACE,
            window,
            property,
            t.ty,
            &t.data[t.offset..end],
        );
        let _ = self.conn.flush();
        if end == t.offset {
            self.incr.remove(pos);
        } else {
            t.offset = end;
        }
    }
}

fn target_list(a: &Atoms, p: &Payload) -> Vec<Atom> {
    match p {
        Payload::Text(_) => vec![a.TARGETS, a.UTF8_STRING, a.TEXT_PLAIN_UTF8, a.TEXT_PLAIN, a.STRING],
        Payload::Files(_) => vec![
            a.TARGETS,
            a.GNOME_FILES,
            a.URI_LIST,
            a.UTF8_STRING,
            a.TEXT_PLAIN_UTF8,
            a.TEXT_PLAIN,
        ],
        Payload::Image(_) => vec![a.TARGETS, a.IMAGE_PNG],
    }
}

fn encode(a: &Atoms, p: &Payload, target: Atom) -> Option<Vec<u8>> {
    let is_text = target == a.UTF8_STRING || target == a.TEXT_PLAIN_UTF8 || target == a.TEXT_PLAIN;
    match p {
        Payload::Text(t) => {
            if is_text {
                Some(t.clone().into_bytes())
            } else if target == a.STRING {
                Some(t.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect())
            } else {
                None
            }
        }
        Payload::Files(f) => {
            if target == a.URI_LIST {
                Some(f.iter().map(|p| path_to_uri(p) + "\r\n").collect::<String>().into_bytes())
            } else if target == a.GNOME_FILES {
                let uris: Vec<String> = f.iter().map(|p| path_to_uri(p)).collect();
                Some(format!("copy\n{}", uris.join("\n")).into_bytes())
            } else if is_text {
                Some(join_paths(f).into_bytes())
            } else {
                None
            }
        }
        Payload::Image(b) => (target == a.IMAGE_PNG).then(|| b.clone()),
    }
}

fn parse_uri_list(data: &[u8], gnome: bool) -> Option<Payload> {
    let text = String::from_utf8_lossy(data);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    if gnome {
        lines.next(); // "copy" or "cut"
    }
    let mut files = Vec::new();
    for l in lines {
        files.push(uri_to_path(l)?); // any non-file URI → treat as plain text
    }
    (!files.is_empty()).then_some(Payload::Files(files))
}
