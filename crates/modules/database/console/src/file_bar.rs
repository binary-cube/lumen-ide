//! The bar above a project's own SQL file (39n): "No data source — Attach…"
//! until the file is attached to a source, then the console's bar on it ([`ConsoleBar`]:
//! Run, the transaction, the schema, the results in the Services window, completion and
//! checks from the source's schema), with the source it runs on beside the schema — its
//! menu attaches the file to another source or detaches it.
//!
//! The attachment is the project's ([`Consoles::attach`], kept in its `sql-files.toml`), so
//! every tab of the file follows it: the bar looks again when the attachments change
//! ([`Consoles::attachments`], one number read), never on each frame otherwise.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use database::DataSourceId;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::ds::{self, Ctx, radius};
use ui_components::editor_toolbar::{
    EditorState, EditorToolbar, StatementMark, ToolbarAction, ToolbarPaint,
};
use ui_components::{Palette, Tip, ViewMenuEntry, ViewerCursor};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::bar::{ConsoleBar, HEIGHT};
use crate::consoles::Consoles;

/// A project's SQL file's bar (see the module's doc).
pub struct SqlFileBar {
    consoles: Arc<Consoles>,
    path: PathBuf,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The attachments' number last looked at.
    seen: Option<u64>,
    state: State,
}

enum State {
    /// No source: "No data source — Attach…".
    Detached(NoSource),
    /// Attached: the console's bar on its source.
    Attached(Box<ConsoleBar>),
}

/// The bar of a file attached to nothing.
#[derive(Default)]
struct NoSource {
    /// The Attach button, as last drawn.
    attach: Option<Rect>,
    hover: bool,
    notifications: Vec<Notification>,
}

impl std::fmt::Debug for SqlFileBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlFileBar")
            .field("path", &self.path)
            .field("attached", &matches!(self.state, State::Attached(_)))
            .finish_non_exhaustive()
    }
}

impl SqlFileBar {
    /// File `path`'s bar (attached or not: as the project keeps it); nothing connects.
    pub fn new(consoles: Arc<Consoles>, path: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let mut bar = Self {
            consoles,
            path: path.to_path_buf(),
            wake,
            seen: None,
            state: State::Detached(NoSource::default()),
        };
        bar.resolve();
        bar
    }

    /// The source it's attached to (`None`: none).
    pub fn source(&self) -> Option<DataSourceId> {
        match &self.state {
            State::Attached(bar) => Some(bar.source()),
            State::Detached(_) => None,
        }
    }

    /// The console's bar it shows, once attached.
    pub fn console_bar(&self) -> Option<&ConsoleBar> {
        match &self.state {
            State::Attached(bar) => Some(bar),
            State::Detached(_) => None,
        }
    }

    /// Looks at the attachment again when the attachments changed: the console's bar made
    /// for a new source (the old one let go), "No data source" once detached. `true` if
    /// it changed.
    fn resolve(&mut self) -> bool {
        let now = self.consoles.attachments();
        if self.seen == Some(now) {
            return false;
        }
        self.seen = Some(now);
        match (&self.state, self.consoles.attached_of(&self.path)) {
            (State::Attached(bar), Some(spec))
                if bar.console() == spec.id && bar.source() == spec.source =>
            {
                false
            }
            (_, Some(spec)) => {
                let runner = self.consoles.runner(spec.id, spec.source);
                runner.set_wake(Arc::clone(&self.wake));
                let bar = ConsoleBar::new(Arc::clone(&self.consoles), spec, runner);
                self.state = State::Attached(Box::new(bar));
                true
            }
            (State::Detached(_), None) => false,
            (State::Attached(_), None) => {
                self.state = State::Detached(NoSource::default());
                true
            }
        }
    }

    /// The Attach button's menu: the sources speaking SQL, or why there are none.
    fn attach_menu(&self) -> Vec<ViewMenuEntry> {
        let sources: Vec<_> = self
            .consoles
            .access()
            .map(|a| a.sources())
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.dialect)
            .collect();
        if sources.is_empty() {
            return vec![ViewMenuEntry::item("none", i18n::t("console.no_sql_sources"))];
        }
        sources
            .into_iter()
            .map(|s| ViewMenuEntry::item(format!("attach:{}", s.id), s.name).with_icon(s.icon))
            .collect()
    }

    fn paint_detached(&mut self, ctx: ToolbarPaint<'_>) {
        let ToolbarPaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let mid = (b.min_y() + b.size.height / 2.0).round();
        ds::divider(layer, &ds, b.min_x(), b.max_x(), b.max_y() - ds.line());
        let font = ds.ui_font(-1.0);
        let State::Detached(state) = &mut self.state else { return };
        let mut x = b.min_x() + px(12.0);
        let s = px(14.0);
        if let Some(id) = icons.id("database") {
            draw_icon(
                layer,
                icons,
                id,
                Rect::new(x.round(), (mid - s / 2.0).round(), s, s),
                p.text_muted,
            );
        }
        x += s + px(8.0);
        let label = text.shape_line(&i18n::t("console.no_source"), &font);
        let base = ds::optical_baseline(mid - px(13.0), px(26.0), &font);
        draw_text(layer, &label, x.round(), base, p.text_muted);
        x += label.width + px(12.0);
        // "Attach Data Source… ▾", a pill as the console bar's.
        let shaped = text.shape_line(&i18n::t("console.attach"), &font);
        let (pad, chevron) = (px(10.0), px(12.0));
        let w = (pad * 2.0 + px(18.0) + shaped.width + chevron + px(4.0)).round();
        let r = Rect::new(x.round(), (mid - px(13.0)).round(), w, px(26.0));
        let fill = if state.hover { p.accent.with_alpha(0.16) } else { p.accent.with_alpha(0.10) };
        layer.quad(
            Quad::filled(r, fill)
                .rounded(px(radius::SM))
                .bordered(ds.line(), p.accent.with_alpha(0.45)),
        );
        let s = px(13.0);
        if let Some(id) = icons.id("link") {
            let ir = Rect::new((r.min_x() + pad).round(), (mid - s / 2.0).round(), s, s);
            draw_icon(layer, icons, id, ir, p.accent);
        }
        draw_text(layer, &shaped, (r.min_x() + pad + px(18.0)).round(), base, p.text);
        if let Some(id) = icons.id("chevron-down") {
            let ir = Rect::new(
                (r.max_x() - pad - chevron + px(3.0)).round(),
                (mid - chevron / 2.0).round(),
                chevron,
                chevron,
            );
            draw_icon(layer, icons, id, ir, p.text_muted);
        }
        state.attach = Some(r);
    }
}

impl EditorToolbar for SqlFileBar {
    fn height(&self) -> f32 {
        HEIGHT
    }

    fn paint(&mut self, ctx: ToolbarPaint<'_>) {
        self.resolve();
        match &mut self.state {
            State::Attached(bar) => bar.paint(ctx),
            State::Detached(_) => self.paint_detached(ctx),
        }
    }

    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        if let State::Attached(bar) = &mut self.state {
            return bar.pointer_down(p);
        }
        let State::Detached(state) = &self.state else { return None };
        let anchor = state.attach.filter(|r| r.contains(p))?;
        Some(ToolbarAction::Menu { anchor, entries: self.attach_menu() })
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        match &mut self.state {
            State::Attached(bar) => bar.hover(p),
            State::Detached(state) => {
                let on = p.is_some_and(|p| state.attach.is_some_and(|r| r.contains(p)));
                std::mem::replace(&mut state.hover, on) != on
            }
        }
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match &self.state {
            State::Attached(bar) => bar.cursor_at(p),
            State::Detached(state) => {
                state.attach.is_some_and(|r| r.contains(p)).then_some(ViewerCursor::Pointer)
            }
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        match &self.state {
            State::Attached(bar) => bar.tooltip_at(p),
            State::Detached(state) => {
                let r = state.attach.filter(|r| r.contains(p))?;
                Some(Tip::new(r, i18n::t("console.tip_attach")))
            }
        }
    }

    fn poll(&mut self) -> bool {
        let changed = self.resolve();
        match &mut self.state {
            State::Attached(bar) => bar.poll() || changed,
            State::Detached(_) => changed,
        }
    }

    fn is_loading(&self) -> bool {
        matches!(&self.state, State::Attached(bar) if bar.is_loading())
    }

    fn title(&self) -> Option<String> {
        None
    }

    fn icon(&self) -> Option<&'static str> {
        None
    }

    fn accent(&self, palette: &Palette) -> Option<Rgba> {
        match &self.state {
            State::Attached(bar) => bar.accent(palette),
            State::Detached(_) => None,
        }
    }

    fn tab_badge(&self, palette: &Palette) -> Option<(String, Rgba)> {
        match &self.state {
            State::Attached(bar) => bar.tab_badge(palette),
            State::Detached(_) => None,
        }
    }

    fn may_close(&mut self) -> bool {
        match &mut self.state {
            State::Attached(bar) => bar.may_close(),
            State::Detached(_) => true,
        }
    }

    fn moved(&mut self, path: &Path) -> bool {
        let kept = match &mut self.state {
            State::Attached(bar) => bar.moved(path),
            // Still a project's SQL file: the same bar (attached there, maybe).
            State::Detached(_) => {
                database::console::attachable(self.consoles.root(), path).is_some()
            }
        };
        if kept {
            self.path = path.to_path_buf();
            self.seen = None;
            self.resolve();
        }
        kept
    }

    fn command(&mut self, id: &str) -> bool {
        let done = match &mut self.state {
            State::Attached(bar) => bar.command(id),
            State::Detached(state) => {
                if let Some(source) = id.strip_prefix("attach:").and_then(DataSourceId::parse) {
                    if self.consoles.attach(&self.path, source).is_none() {
                        state
                            .notifications
                            .push(Notification::warning(i18n::t("console.not_attachable")));
                        return false;
                    }
                    true
                } else {
                    id == "none"
                }
            }
        };
        self.resolve();
        done
    }

    fn take_action(&mut self) -> Option<ToolbarAction> {
        match &mut self.state {
            State::Attached(bar) => bar.take_action(),
            State::Detached(_) => None,
        }
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        match &mut self.state {
            State::Attached(bar) => bar.take_notifications(),
            State::Detached(state) => std::mem::take(&mut state.notifications),
        }
    }

    fn follows_editor(&self) -> bool {
        matches!(&self.state, State::Attached(bar) if bar.follows_editor())
    }

    fn sync_editor(&mut self, state: &EditorState) {
        if let State::Attached(bar) = &mut self.state {
            bar.sync_editor(state);
        }
    }

    fn marks(&self) -> (&[StatementMark], u64) {
        match &self.state {
            State::Attached(bar) => bar.marks(),
            State::Detached(_) => (&[], 0),
        }
    }

    fn highlight(&self) -> Option<std::ops::Range<usize>> {
        match &self.state {
            State::Attached(bar) => bar.highlight(),
            State::Detached(_) => None,
        }
    }

    fn focus(&self) -> Option<(std::ops::Range<usize>, ui_components::Level)> {
        match &self.state {
            State::Attached(bar) => bar.focus(),
            State::Detached(_) => None,
        }
    }

    fn purge(&mut self) {
        if let State::Attached(bar) = &mut self.state {
            bar.purge();
        }
    }

    fn grammar_hints(&self) -> bool {
        matches!(&self.state, State::Attached(bar) if bar.grammar_hints())
    }
}

#[cfg(test)]
#[path = "../tests/unit/file_bar.rs"]
mod tests;
