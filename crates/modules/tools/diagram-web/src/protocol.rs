//! What the view and draw.io's page say to each other (its embed protocol, as JSON), as a
//! state machine with no I/O: the page's messages, the file's reads and writes and the
//! disk's changes go in, what to do comes out ([`Effect`]: a message for the page, a write,
//! closing the tab, a link to open).
//!
//! - `configure` → our configuration (the theme's colors, 82g); `init` → the file's text
//!   (`load`, autosaving), as soon as it's read — again after the page is made anew.
//! - `autosave` / `save` → the text written, one write at a time, the latest text waiting
//!   (never an older one after it); then `status` (nothing unsaved).
//! - The file changed on disk by someone else, nothing unsaved here → `load`ed again (a
//!   `merge` needs a file of draw.io's own, which embed mode doesn't always have: it was
//!   let go silently when tried live).
//! - `export` (its exports, asked by `exportProtocol` instead of a download the web view
//!   refuses) → the bytes, to be saved where the user chooses.
//! - `exit` → the tab closes; `openLink` → the link, if it's the web's.
//! - Our page's own (`host.js`, [`crate::state`]): `hostTouched` → the user acted on it;
//!   `hostState` → where the user is, kept and
//!   given back after every `load` (`hostRestore`, with whether an undo or redo here is
//!   offered); `hostUndo` / `hostRedo` → a step back or forward past the page's own history,
//!   loaded and written.
//! - Anything else, malformed or too large, is let go.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::state::{History, ViewState};

/// The largest message read from the page (bytes): a diagram with pictures is a few MB.
pub const MAX_MESSAGE: usize = 64 << 20;

/// What the view does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// This text to the page.
    Send(String),
    /// The file's text written as this (on a worker; [`Session::written`] when done).
    Write(Arc<str>),
    /// The tab closes.
    Close,
    /// A web link opened in the system's browser.
    OpenLink(String),
    /// An export to save (where the user chooses): its suggested name, its bytes.
    SaveAs { name: String, bytes: Vec<u8> },
    /// The user acted on the page (it's in use: not put to sleep).
    Touched,
}

/// One page's conversation about one file.
#[derive(Debug, Default)]
pub struct Session {
    /// The configuration sent when the page asks (a JSON object).
    config: Value,
    /// The tab's title, shown by the page.
    title: String,
    /// The diagram as last known: read from disk, or as the page sent it.
    xml: Option<Arc<str>>,
    /// What the disk holds as far as we know (read, or written by us): a change to it that
    /// is this is our own.
    on_disk: Option<Arc<str>>,
    /// The page said `init` (it takes `load` and `merge`).
    ready: bool,
    /// A write is under way.
    writing: bool,
    /// The text waiting for the write under way to end (the latest only).
    queued: Option<Arc<str>>,
    /// The last write failed (what's in the page isn't on disk).
    failed: bool,
    /// Where the user was in the page, as it last said.
    view: Option<ViewState>,
    /// What the diagram was before each change (undo past a page made anew).
    history: History,
}

impl Session {
    pub fn new(title: impl Into<String>, config: Value) -> Self {
        Self { title: title.into(), config, ..Self::default() }
    }

    /// What the disk held when last read or written (a write checks it's still that).
    pub fn on_disk(&self) -> Option<Arc<str>> {
        self.on_disk.clone()
    }

    /// The text to save now (the latest the page sent), if it isn't on disk.
    pub fn unsaved(&self) -> Option<Arc<str>> {
        let latest = self.queued.clone().or_else(|| self.xml.clone())?;
        (self.on_disk.as_deref() != Some(&*latest) || self.failed).then_some(latest)
    }

    /// A write found the file changed by someone else (`disk`, kept as it is; ours went
    /// beside it): what waited is let go, the page shows the file as it is now.
    pub fn conflict(&mut self, disk: Arc<str>) -> Vec<Effect> {
        self.writing = false;
        self.queued = None;
        self.failed = false;
        self.on_disk = Some(Arc::clone(&disk));
        self.xml = Some(disk);
        if self.ready { self.load() } else { Vec::new() }
    }

    /// Something's unsaved: a write under way or waiting, or one that failed.
    pub fn dirty(&self) -> bool {
        self.writing || self.queued.is_some() || self.failed
    }

    /// The page takes messages (it said `init`).
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Something is being typed into a cell, or a dialog is open (as the page last said).
    pub fn busy(&self) -> bool {
        self.view.as_ref().is_some_and(|v| v.busy)
    }

    /// The diagram as last known.
    pub fn xml(&self) -> Option<&Arc<str>> {
        self.xml.as_ref()
    }

    /// Another configuration, for the next page (a theme changed).
    pub fn set_config(&mut self, config: Value) {
        self.config = config;
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// The page is made anew (another id): it will ask again; its own history is gone, ours
    /// takes over from here.
    pub fn new_page(&mut self) {
        self.ready = false;
        self.history.page_made();
    }

    /// What the page said.
    pub fn page_message(&mut self, text: &str) -> Vec<Effect> {
        if text.len() > MAX_MESSAGE {
            return Vec::new();
        }
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        let Some(event) = message.get("event").and_then(Value::as_str) else { return Vec::new() };
        match event {
            "configure" => {
                // A page starting (made again, or reloaded on its own): its own history gone,
                // ours takes over from here.
                self.ready = false;
                self.history.page_made();
                vec![Effect::Send(
                    json!({"action": "configure", "config": self.config}).to_string(),
                )]
            }
            "init" => {
                self.ready = true;
                self.load()
            }
            "autosave" | "save" => {
                let Some(xml) = message.get("xml").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let mut out = self.edited(Arc::from(xml));
                if event == "save" && message.get("exit").and_then(Value::as_bool) == Some(true) {
                    out.push(Effect::Close);
                }
                out
            }
            "hostState" => {
                if let Some(view) = ViewState::read(&message) {
                    self.view = Some(view);
                }
                Vec::new()
            }
            "hostTouched" => vec![Effect::Touched],
            "hostUndo" => self.step(true),
            "hostRedo" => self.step(false),
            "exit" => vec![Effect::Close],
            "export" => export(&message).map(|e| vec![e]).unwrap_or_default(),
            "openLink" => message
                .get("href")
                .and_then(Value::as_str)
                .filter(|href| web_link(href))
                .map(|href| vec![Effect::OpenLink(href.to_owned())])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The `load` message for what's known (if it is), then where the user was and what can be
    /// undone here (our page keeps it until the diagram has loaded).
    fn load(&self) -> Vec<Effect> {
        let Some(xml) = self.xml.as_ref() else { return Vec::new() };
        let load = json!({
            "action": "load",
            "xml": &**xml,
            "autosave": 1,
            "title": self.title,
            "modified": 0,
            // Exports come to us as messages, never as a download.
            "exportProtocol": true,
        });
        let (past, future) = self.history.can();
        let restore = json!({
            "action": "hostRestore",
            "state": self.view.as_ref().map(ViewState::to_json),
            "past": past,
            "future": future,
        });
        vec![Effect::Send(load.to_string()), Effect::Send(restore.to_string())]
    }

    /// A step back (`back`) or forward past the page's own history: loaded where the user is,
    /// and written.
    fn step(&mut self, back: bool) -> Vec<Effect> {
        let Some(now) = self.xml.clone() else { return Vec::new() };
        let to = if back { self.history.undo(now) } else { self.history.redo(now) };
        let Some(to) = to else { return Vec::new() };
        self.xml = Some(Arc::clone(&to));
        let mut out = if self.ready { self.load() } else { Vec::new() };
        out.extend(self.write_latest(to));
        out
    }

    /// The page's diagram as it sent it: what it was kept (to undo past a page made anew),
    /// then written.
    fn edited(&mut self, xml: Arc<str>) -> Vec<Effect> {
        let was = self.xml.replace(Arc::clone(&xml));
        if let Some(was) = was.filter(|was| **was != *xml) {
            self.history.changed(was);
        }
        self.write_latest(xml)
    }

    /// `xml`, the latest, written (now, or after the write under way).
    fn write_latest(&mut self, xml: Arc<str>) -> Vec<Effect> {
        if !self.writing && !self.failed && self.on_disk.as_deref() == Some(&*xml) {
            return Vec::new();
        }
        if self.writing {
            self.queued = Some(xml);
            return Vec::new();
        }
        self.writing = true;
        vec![Effect::Write(xml)]
    }

    /// The file was read (its first read): loaded if the page waits for it.
    pub fn file_read(&mut self, xml: Arc<str>) -> Vec<Effect> {
        self.on_disk = Some(Arc::clone(&xml));
        self.xml = Some(xml);
        if self.ready { self.load() } else { Vec::new() }
    }

    /// The write of `xml` ended (`ok`: it's on disk): the next one waiting starts, else the
    /// page is told nothing's unsaved.
    pub fn written(&mut self, xml: &Arc<str>, ok: bool) -> Vec<Effect> {
        self.writing = false;
        self.failed = !ok;
        if ok {
            self.on_disk = Some(Arc::clone(xml));
        }
        if let Some(next) = self.queued.take() {
            self.writing = true;
            self.failed = false;
            return vec![Effect::Write(next)];
        }
        if ok && self.ready {
            return vec![Effect::Send(json!({"action": "status", "modified": false}).to_string())];
        }
        Vec::new()
    }

    /// The file changed on disk to `xml` (read after the disk said so): our own write
    /// changes nothing; someone else's is merged into the page when nothing's unsaved here
    /// (else what's here is kept, and written over it at the next save).
    pub fn disk_changed(&mut self, xml: Arc<str>) -> Vec<Effect> {
        if self.on_disk.as_deref() == Some(&*xml) || self.xml.as_deref() == Some(&*xml) {
            self.on_disk = Some(xml);
            return Vec::new();
        }
        if self.dirty() {
            return Vec::new();
        }
        self.on_disk = Some(Arc::clone(&xml));
        self.xml = Some(Arc::clone(&xml));
        if !self.ready {
            return Vec::new();
        }
        self.load()
    }
}

/// An `export` event's file: its name (a plain one) and its bytes (a `data:` URL in base64,
/// else the text itself).
fn export(message: &serde_json::Map<String, Value>) -> Option<Effect> {
    let data = message.get("data")?.as_str()?;
    let format = message.get("format").and_then(Value::as_str).unwrap_or("");
    let given = message.get("filename").and_then(Value::as_str).unwrap_or("");
    let bytes = match data.strip_prefix("data:") {
        Some(url) => {
            let (head, body) = url.split_once(',')?;
            if head.ends_with(";base64") { base64_decode(body)? } else { body.as_bytes().to_vec() }
        }
        None => data.as_bytes().to_vec(),
    };
    Some(Effect::SaveAs { name: file_name(given, format), bytes })
}

/// A name usable as one file: the last part of `given`, no control or reserved characters,
/// bounded; `diagram.<format>` when nothing's left.
pub fn file_name(given: &str, format: &str) -> String {
    let last = given.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .take(120)
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_owned();
    if clean.is_empty() {
        let ext: String = format.chars().filter(char::is_ascii_alphanumeric).take(8).collect();
        format!("diagram.{}", if ext.is_empty() { "bin" } else { &ext })
    } else {
        clean
    }
}

/// Standard base64 (padding and whitespace allowed); `None`: not base64.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Whether `href` is a web link (`http`/`https`, nothing else: no `file:`, no `javascript:`).
pub fn web_link(href: &str) -> bool {
    let lower = href.trim_start().get(..8).unwrap_or("").to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !href.chars().any(char::is_control)
}

#[cfg(test)]
#[path = "../tests/unit/protocol.rs"]
mod tests;
