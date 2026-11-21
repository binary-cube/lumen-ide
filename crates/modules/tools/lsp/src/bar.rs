//! The bar over a file whose language's server isn't downloaded yet (an editor bar, the
//! app's extension point for them): "TypeScript: for full completion and checks I'd download
//! vtsls + Node.js (~45 MB, once). [Download] [Not now] [Use the built-in engine]". The file
//! works on the built-in engine meanwhile; once the download ends the server starts by
//! itself (every project's documents go to their engines again) and the bar goes.
//!
//! - Download: the components missing (Node only when it is), one after the other in the
//!   background, their progress here and in the Task Manager; Stop; a failure (or no
//!   network) said, with Retry.
//! - Not now: the language's bars go until the app starts again.
//! - Use the built-in engine: saved as the language's engine (as from the settings).
//!
//! The bar takes no keys and never the focus; it works out what it shows only when the
//! downloads, the settings or the dismissals changed (a number each), not at each frame.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx};
use ui_components::editor_toolbar::{
    EditorToolbar, EditorToolbarProvider, HEIGHT, ToolbarAction, ToolbarContext, ToolbarPaint,
};
use ui_components::{Tip, ViewerAction, ViewerCursor};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::report::language_name;
use crate::{catalog, downloads, settings};

/// The command saving the built-in engine for the active file's language.
pub const USE_BUILT_IN: &str = "lsp.use_built_in";

/// The languages whose bars the user sent away this session ("Not now").
static DISMISSED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Language `language`'s bars go until the app starts again.
pub fn dismiss(language: &str) {
    if let Ok(mut d) = DISMISSED.lock() {
        d.get_or_insert_with(HashSet::new).insert(language.to_owned());
    }
    crate::install::bump();
}

/// Every language offered again (tests).
#[cfg(test)]
pub(crate) fn forget_dismissals() {
    if let Ok(mut d) = DISMISSED.lock() {
        *d = None;
    }
}

fn dismissed(language: &str) -> bool {
    DISMISSED.lock().is_ok_and(|d| d.as_ref().is_some_and(|d| d.contains(language)))
}

/// What's offered for a language now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The server it's for (the first choice's).
    pub server: String,
    /// The components to download (their names), Node and PHP first.
    pub components: Vec<String>,
    /// How they're called ("vtsls + Node.js": the server's first).
    pub parts: String,
    /// What they'd download (bytes).
    pub size: u64,
    /// The servers the user can pick between (the language describes several it can get:
    /// PHP's intelephense, installed, and phpactor, downloaded), the default first; one:
    /// just "Download".
    pub choices: Vec<OfferChoice>,
}

/// A server the bar offers: what it needs (downloads, or an install of one the user
/// installs — its licence agreed by the click).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferChoice {
    pub server: String,
    pub components: Vec<String>,
    pub parts: String,
    pub size: u64,
    /// It's installed by the user (proprietary: its licence shown with it).
    pub install: bool,
}

/// What server `spec` needs before it can answer (`None`: nothing).
fn choice_for(spec: &lsp_client::spec::ServerSpec) -> Option<OfferChoice> {
    let components = downloads::needed_for(spec);
    // One the user installs, only where this build downloads (its runtime comes so).
    let install =
        spec.install.is_some() && !crate::install::available(spec) && downloads::root().is_some();
    if components.is_empty() && !install {
        return None;
    }
    let manifest = downloads::manifest();
    let label = |name: &String| manifest.named(name).map_or_else(|| name.clone(), |c| c.label());
    // The server first ("vtsls + Node.js"), its runtime after.
    let mut labels: Vec<String> = components.iter().rev().map(label).collect();
    if install {
        labels.insert(0, spec.name.clone());
    }
    labels.dedup();
    let own = if install { spec.install.as_ref().and_then(|i| i.size).unwrap_or(0) } else { 0 };
    Some(OfferChoice {
        server: spec.name.clone(),
        size: downloads::size_of(&components) + own,
        parts: labels.join(" + "),
        components,
        install,
    })
}

/// What the bar of a `language` file offers now (`None`: nothing — its engine isn't a
/// server, its server is there, can't be downloaded, or the user said "Not now"). Where the
/// user hasn't chosen, every server the language describes as answering here by default is
/// offered (the first: the default once it's there).
pub fn offer(language: &str) -> Option<Offer> {
    if dismissed(language) {
        return None;
    }
    let config = settings::config();
    let explicit = config.engines.iter().any(|(l, _)| l == language);
    let choice = config.choice(language);
    let catalog = catalog::catalog();
    let usable = |s: &&lsp_client::spec::ServerSpec| !config.disabled.contains(&s.name);
    let choices: Vec<OfferChoice> = if explicit {
        if !choice.engine.uses_server() {
            return None;
        }
        let spec = choice
            .server
            .as_deref()
            .and_then(|name| catalog.for_language(language).find(|s| s.name == name))
            .or_else(|| catalog.for_language(language).find(usable))?;
        if config.disabled.contains(&spec.name) {
            return None;
        }
        vec![choice_for(spec)?]
    } else {
        let here: Vec<&lsp_client::spec::ServerSpec> = catalog
            .for_language(language)
            .filter(usable)
            .filter(|s| s.default_engine() != code_intelligence::servers::Engine::BuiltIn)
            .collect();
        // One of them can answer already: nothing to offer.
        if here.iter().any(|s| choice_for(s).is_none()) {
            return None;
        }
        here.into_iter().filter_map(choice_for).collect()
    };
    let first = choices.first()?.clone();
    Some(Offer {
        server: first.server.clone(),
        components: first.components.clone(),
        parts: first.parts.clone(),
        size: first.size,
        choices,
    })
}

/// The bars' provider: a bar for a file of a language with something to download.
#[derive(Debug, Default)]
pub struct OfferProvider;

impl EditorToolbarProvider for OfferProvider {
    fn id(&self) -> &'static str {
        "language-server-download"
    }

    fn open(&self, context: &ToolbarContext<'_>) -> Option<Box<dyn EditorToolbar>> {
        let language = context.language?;
        offer(language)?;
        Some(Box::new(DownloadBar::new(language, Arc::clone(&context.wake))))
    }
}

/// What a bar shows.
#[derive(Debug, Clone, PartialEq)]
enum View {
    Offer(Offer),
    Downloading { offer: Offer, received: u64, total: u64, waiting: bool },
    Failed { offer: Offer, reason: String, offline: bool },
}

impl View {
    fn offer(&self) -> &Offer {
        match self {
            Self::Offer(o) | Self::Downloading { offer: o, .. } | Self::Failed { offer: o, .. } => {
                o
            }
        }
    }
}

/// A bar's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The bar's choice `i` (a language with several servers it can get).
    Choose(usize),
    Download,
    NotNow,
    BuiltIn,
    Stop,
    Retry,
}

/// A file's bar (see the module's doc).
pub struct DownloadBar {
    language: String,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The numbers last looked at (downloads, settings).
    seen: Option<(u64, u64)>,
    view: Option<View>,
    /// Its buttons as last drawn.
    buttons: Vec<(Rect, Button)>,
    hovered: Option<Button>,
    /// It started the download (it tells when it's done).
    started: Option<String>,
    notifications: Vec<Notification>,
    action: Option<ToolbarAction>,
}

impl std::fmt::Debug for DownloadBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadBar")
            .field("language", &self.language)
            .field("view", &self.view)
            .finish_non_exhaustive()
    }
}

impl DownloadBar {
    pub fn new(language: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let mut bar = Self {
            language: language.to_owned(),
            wake,
            seen: None,
            view: None,
            buttons: Vec::new(),
            hovered: None,
            started: None,
            notifications: Vec::new(),
            action: None,
        };
        bar.resolve();
        bar
    }

    /// Whether it shows anything.
    pub fn shown(&self) -> bool {
        self.view.is_some()
    }

    /// Works out what it shows again when something changed: `true` if it did.
    fn resolve(&mut self) -> bool {
        let now = (crate::install::generation(), settings::generation());
        if self.seen == Some(now) {
            return false;
        }
        self.seen = Some(now);
        let view = offer(&self.language).map(|offer| {
            let mut received = 0;
            let mut total = 0;
            let mut waiting = false;
            let mut going = false;
            let mut failed = None;
            for name in &offer.components {
                match downloads::status(name) {
                    downloads::Status::Downloading(p) => {
                        going = true;
                        received += p.received;
                        total += p.total.max(p.received);
                        waiting |= p.waiting;
                    }
                    downloads::Status::Missing { size } => {
                        total += size;
                        failed = failed.or_else(|| downloads::failure(name));
                    }
                    _ => {}
                }
            }
            // A server the user installs, being installed (from the bar or the settings).
            let installing = offer
                .choices
                .iter()
                .filter(|c| c.install)
                .filter_map(|c| catalog::catalog().named(&c.server).cloned())
                .find_map(|spec| crate::install::state(&spec).installing);
            if let Some((got, size)) = installing {
                going = true;
                received += got;
                total += size.unwrap_or(got).max(got);
            }
            match (going, failed) {
                (true, _) => View::Downloading { offer, received, total, waiting },
                (false, Some(f)) => View::Failed { offer, reason: f.reason, offline: f.offline },
                (false, None) => View::Offer(offer),
            }
        });
        // It started the download and it's done: said once.
        if view.is_none()
            && let Some(server) = self.started.take()
        {
            self.notifications.push(Notification::success(tr!(
                "lsp.offer_done",
                language = language_name(&self.language),
                server = server
            )));
        }
        let changed = view != self.view;
        self.view = view;
        if self.view.is_none() {
            self.buttons.clear();
            self.hovered = None;
        }
        changed
    }

    /// The text it shows.
    pub fn text(&self) -> String {
        let language = language_name(&self.language);
        match &self.view {
            None => String::new(),
            Some(View::Offer(o)) if o.choices.len() > 1 => {
                tr!("lsp.offer_choose", language = language.as_str())
            }
            Some(View::Offer(o)) if o.choices.first().is_some_and(|c| c.install) => tr!(
                "lsp.offer_install_one",
                language = language.as_str(),
                parts = o.parts.as_str(),
                size = i18n::bytes(o.size)
            ),
            Some(View::Offer(o)) => tr!(
                "lsp.offer",
                language = language.as_str(),
                parts = o.parts.as_str(),
                size = i18n::bytes(o.size)
            ),
            Some(View::Downloading { offer, waiting: true, .. }) => {
                tr!("lsp.offer_waiting", language = language.as_str(), parts = offer.parts.as_str())
            }
            Some(View::Downloading { offer, received, total, .. }) => tr!(
                "lsp.offer_downloading",
                language = language.as_str(),
                parts = offer.parts.as_str(),
                received = i18n::bytes(*received),
                total = i18n::bytes(*total)
            ),
            Some(View::Failed { offer, offline: true, .. }) => {
                tr!("lsp.offer_offline", language = language.as_str(), parts = offer.parts.as_str())
            }
            Some(View::Failed { offer, reason, .. }) => tr!(
                "lsp.offer_failed",
                language = language.as_str(),
                parts = offer.parts.as_str(),
                reason = reason.as_str()
            ),
        }
    }

    /// The buttons it shows, in order.
    pub fn button_list(&self) -> Vec<Button> {
        match &self.view {
            None => Vec::new(),
            Some(View::Offer(o))
                if o.choices.len() > 1 || o.choices.first().is_some_and(|c| c.install) =>
            {
                let mut buttons: Vec<Button> = (0..o.choices.len()).map(Button::Choose).collect();
                buttons.extend([Button::NotNow, Button::BuiltIn]);
                buttons
            }
            Some(View::Offer(_)) => vec![Button::Download, Button::NotNow, Button::BuiltIn],
            Some(View::Downloading { .. }) => vec![Button::Stop],
            Some(View::Failed { .. }) => vec![Button::Retry, Button::NotNow, Button::BuiltIn],
        }
    }

    fn label(&self, button: Button) -> String {
        if let Button::Choose(i) = button {
            let choice = self.view.as_ref().and_then(|v| v.offer().choices.get(i).cloned());
            return choice.map_or_else(String::new, |c| {
                tr!("lsp.offer_choice", server = c.server.as_str(), size = i18n::bytes(c.size))
            });
        }
        t(match button {
            Button::Choose(_) => "lsp.offer_download",
            Button::Download => "lsp.offer_download",
            Button::NotNow => "lsp.offer_not_now",
            Button::BuiltIn => "lsp.offer_built_in",
            Button::Stop => "lsp.offer_stop",
            Button::Retry => "lsp.offer_retry",
        })
    }

    /// Button `button` pressed: what it does.
    pub fn press(&mut self, button: Button) -> Option<ToolbarAction> {
        let offer = self.view.as_ref().map(|v| v.offer().clone())?;
        match button {
            Button::Choose(i) => {
                let choice = offer.choices.get(i)?;
                let (language, server) = (self.language.clone(), choice.server.clone());
                self.started = Some(server.clone());
                let work = ui_components::WithHost::once(move |host| {
                    crate::engines::pick_offered(host, &language, server);
                });
                return Some(ToolbarAction::View(ViewerAction::WithHost(work)));
            }
            Button::Download | Button::Retry => {
                self.started = Some(offer.server.clone());
                downloads::start(offer.components, Some(Arc::clone(&self.wake)));
            }
            Button::NotNow => dismiss(&self.language),
            Button::BuiltIn => {
                return Some(ToolbarAction::View(ViewerAction::RunCommand(USE_BUILT_IN.into())));
            }
            Button::Stop => {
                for name in &offer.components {
                    downloads::stop(name);
                }
            }
        }
        self.resolve();
        None
    }

    fn button_at(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }
}

impl EditorToolbar for DownloadBar {
    fn height(&self) -> f32 {
        if self.view.is_some() { HEIGHT } else { 0.0 }
    }

    fn paint(&mut self, ctx: ToolbarPaint<'_>) {
        self.resolve();
        let Some(view) = self.view.clone() else { return };
        let ToolbarPaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let tint = match &view {
            View::Failed { .. } => p.warning,
            _ => p.accent,
        };
        layer.quad(Quad::filled(b, tint.with_alpha(0.07)));
        ds::divider(layer, &ds, b.min_x(), b.max_x(), b.max_y() - ds.line());
        let mid = (b.min_y() + b.size.height / 2.0).round();
        let font = ds.ui_font(-1.0);
        // The buttons, from the right.
        let button_font = ds::button_font(&ds);
        let h = px(26.0);
        let mut right = b.max_x() - px(12.0);
        let mut buttons = Vec::new();
        for button in self.button_list().into_iter().rev() {
            let label = self.label(button);
            let w = (text.shape_line(&label, &button_font).width + px(24.0)).round();
            let r = Rect::new((right - w).round(), (mid - h / 2.0).round(), w, h);
            let hovered = self.hovered == Some(button);
            let kind = match button {
                Button::Download | Button::Retry | Button::Choose(0) => ButtonKind::Primary,
                _ => ButtonKind::Secondary,
            };
            let spec = ButtonSpec::new(kind).label(&label).hovered(hovered);
            ds::button(layer, text, icons, &ds, r, spec);
            buttons.push((r, button));
            right = r.min_x() - px(8.0);
        }
        self.buttons = buttons;
        // The icon and the text, cut to what's left.
        let mut x = b.min_x() + px(12.0);
        let s = px(14.0);
        let icon = match &view {
            View::Failed { offline: true, .. } => "cloud",
            View::Failed { .. } => "circle-alert",
            _ => "download",
        };
        if let Some(id) = icons.id(icon).or_else(|| icons.id("download")) {
            let color = if matches!(view, View::Failed { .. }) { p.warning } else { p.accent };
            draw_icon(layer, icons, id, Rect::new(x.round(), (mid - s / 2.0).round(), s, s), color);
        }
        x += s + px(8.0);
        let room = (right - px(8.0) - x).max(0.0);
        // On one line when it fits beside the buttons, else on two (a narrow editor), each
        // cut to what's left.
        let whole = self.text();
        let lines = match text.shape_line(&whole, &font).width <= room {
            true => vec![whole],
            false => wrap_in_two(&whole, |s| text.shape_line(s, &font).width <= room),
        };
        let line_h = px(16.0);
        let top = mid - line_h * lines.len() as f32 / 2.0;
        for (i, part) in lines.iter().enumerate() {
            let shown = ds::ellipsize(text, part, &font, room);
            let line = text.shape_line(&shown, &font);
            let y = ds::optical_baseline(top + line_h * i as f32, line_h, &font);
            draw_text(layer, &line, x.round(), y.round(), p.text);
        }
        // The progress, along its bottom.
        if let View::Downloading { received, total, waiting, .. } = view {
            let track = Rect::new(b.min_x(), b.max_y() - px(2.0), b.size.width, px(2.0));
            let fraction = (!waiting && total > 0).then(|| received as f32 / total as f32);
            ds::ProgressBar::from(fraction).paint_bar(layer, &ds, track);
        }
    }

    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        let button = self.button_at(p)?;
        self.press(button)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let on = p.and_then(|p| self.button_at(p));
        std::mem::replace(&mut self.hovered, on) != on
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.button_at(p).map(|_| ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (r, button) = self.buttons.iter().find(|(r, _)| r.contains(p))?;
        let language = language_name(&self.language);
        let label = match button {
            Button::Choose(i) => {
                let choice = self.view.as_ref()?.offer().choices.get(*i)?.clone();
                // What tells it apart (the language's note for it), and, for one the user
                // installs, its licence: the click is the consent.
                let key = format!("lsp.offer_note_{}", choice.server.replace('-', "_"));
                let note = t(&key);
                let mut text = if note == key { String::new() } else { note };
                if choice.install {
                    let spec = catalog::catalog().named(&choice.server).cloned();
                    let licence = spec
                        .and_then(|s| s.install?.licence_url)
                        .unwrap_or_else(|| crate::install::REGISTRY_NOTE.to_owned());
                    let consent = tr!("lsp.offer_install_tip", licence = licence);
                    text = if text.is_empty() { consent } else { format!("{text} {consent}") };
                }
                if text.is_empty() {
                    tr!(
                        "lsp.offer_tip_download",
                        path =
                            downloads::root().map_or_else(String::new, |r| r.display().to_string())
                    )
                } else {
                    text
                }
            }
            Button::Download | Button::Retry => tr!(
                "lsp.offer_tip_download",
                path = downloads::root().map_or_else(String::new, |r| r.display().to_string())
            ),
            Button::NotNow => t("lsp.offer_tip_not_now"),
            Button::BuiltIn => tr!("lsp.offer_tip_built_in", language = language.as_str()),
            Button::Stop => t("lsp.offer_tip_stop"),
        };
        Some(Tip::new(*r, label))
    }

    fn poll(&mut self) -> bool {
        self.resolve()
    }

    fn is_loading(&self) -> bool {
        // Only an unknown amount sweeps (else the downloads wake it as they go).
        matches!(self.view, Some(View::Downloading { waiting: true, .. }))
    }

    fn take_action(&mut self) -> Option<ToolbarAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn moved(&mut self, _path: &std::path::Path) -> bool {
        // Asked again (the file's language may be another's now).
        false
    }
}

/// `text` on two lines: as many words on the first as `fits` allows (at least one), the
/// rest on the second.
fn wrap_in_two(text: &str, mut fits: impl FnMut(&str) -> bool) -> Vec<String> {
    let words: Vec<&str> = text.split(' ').collect();
    let mut first = 1;
    while first < words.len() && fits(&words[..=first].join(" ")) {
        first += 1;
    }
    let (a, b) = (words[..first].join(" "), words[first..].join(" "));
    if b.is_empty() { vec![a] } else { vec![a, b] }
}

#[cfg(test)]
#[path = "../tests/unit/bar.rs"]
mod tests;
