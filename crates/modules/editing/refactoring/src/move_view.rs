//! The Move preview (a tab): where the file / folder goes — the namespace its classes take
//! (their folder follows it, PSR-4), or the folder for anything else — then every place the
//! move changes, file by file, as the other refactorings show them. The project is scanned
//! once on a background thread; the plan follows the namespace as it's typed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use refactor::Plan;
use refactor::moving::{MoveContext, MoveFacts, plan_move, scan_for_move};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, Tone, radius, space};
use ui_components::{Viewer, ViewerAction, ViewerKey, ViewerPaint};
use ui_kit::{TextInput, TextInputStyle, draw_text};

use crate::page::{MIN_LIST, PageScroll};
use crate::plan_list::PlanList;

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-move";

/// What the view is opened with.
#[derive(Debug, Clone)]
pub struct MoveData {
    pub context: MoveContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Apply,
    Field,
    Close,
}

/// Why the move can't be applied as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    InvalidNamespace,
    /// Already there (nothing to move).
    Same,
    /// Something with that name is already there.
    Exists,
    /// A folder into itself.
    IntoItself,
}

#[derive(Default)]
pub struct MoveView {
    context: Option<MoveContext>,
    /// The namespace (classes) or the folder, relative to the project (anything else).
    field: TextInput,
    facts: Option<Arc<MoveFacts>>,
    scanning: Option<Receiver<MoveFacts>>,
    plan: Option<Plan>,
    cancel: Arc<AtomicBool>,
    action: Option<ViewerAction>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    list: PlanList,
    page: PageScroll,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Whether the destination is taken, by the destination (asked of the disk once per
    /// destination typed, not on every frame).
    taken: std::cell::RefCell<kernel::Memo<PathBuf, bool>>,
}

impl std::fmt::Debug for MoveView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveView").field("field", &self.field.text()).finish_non_exhaustive()
    }
}

impl Drop for MoveView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl MoveView {
    /// Its classes' namespace is typed (else a folder).
    fn by_namespace(&self) -> bool {
        self.context.as_ref().is_some_and(|c| c.rules.is_some() && !c.target.classes.is_empty())
    }

    /// Where the moved entry ends up, as typed.
    pub fn destination(&self) -> Option<PathBuf> {
        let context = self.context.as_ref()?;
        let typed = self.field.text().trim();
        Some(if self.by_namespace() {
            context.target.destination(context.roots.separator.trim(typed), &context.roots)
        } else {
            let folder = typed.trim_matches(|c| c == '/' || c == '\\');
            let dir =
                if folder.is_empty() { context.root.clone() } else { context.root.join(folder) };
            dir.join(context.target.name())
        })
    }

    pub fn problem(&self) -> Option<Problem> {
        let context = self.context.as_ref()?;
        let separator = context.roots.separator;
        let typed = separator.trim(self.field.text().trim());
        if self.by_namespace() && !separator.valid_namespace(typed) {
            return Some(Problem::InvalidNamespace);
        }
        let to = self.destination()?;
        let from = &context.target.from;
        if to == *from {
            // The same place: only the namespace may change.
            let moves = context.target.moves(typed);
            return (!self.by_namespace() || moves.is_empty()).then_some(Problem::Same);
        }
        if to.starts_with(from) {
            return Some(Problem::IntoItself);
        }
        let taken = *self.taken.borrow_mut().get_or_update(to.clone(), || to.exists());
        taken.then_some(Problem::Exists)
    }

    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    pub fn can_apply(&self) -> bool {
        self.problem().is_none()
            && self
                .plan
                .as_ref()
                .is_some_and(|p| p.chosen().0 > 0 || p.renames.iter().any(|r| r.chosen))
    }

    pub fn apply(&self) -> Option<ViewerAction> {
        let mut plan = self.plan.clone()?;
        if !self.can_apply() {
            return None;
        }
        // Moved only if it goes somewhere else.
        plan.renames.retain(|r| r.from != r.to);
        Some(ViewerAction::ApplyRefactoring(plan))
    }

    /// Types `text` into the field.
    pub fn type_text(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        self.field.insert(&text);
        self.replan();
    }

    /// Sets the scanned files (from the background scan, or directly in tests).
    pub fn set_facts(&mut self, facts: MoveFacts) {
        self.facts = Some(Arc::new(facts));
        self.replan();
    }

    /// The plan for what's typed (quick: the files are scanned already).
    fn replan(&mut self) {
        let (Some(context), Some(facts)) = (&self.context, &self.facts) else { return };
        let Some(to) = self.destination() else { return };
        let ns = if self.by_namespace() { self.field.text().trim().trim_matches('\\') } else { "" };
        let mut plan = plan_move(context, facts, ns, &to);
        // The checkboxes chosen by hand stay as they were for the same places.
        if let Some(old) = &self.plan {
            for file in &mut plan.files {
                let Some(before) = old.files.iter().find(|f| f.path == file.path) else { continue };
                for o in &mut file.occurrences {
                    if let Some(b) = before
                        .occurrences
                        .iter()
                        .find(|b| b.range == o.range && b.replacement == o.replacement)
                    {
                        o.chosen = b.chosen;
                    }
                }
            }
        }
        self.plan = Some(plan);
    }

    fn scan(&mut self) {
        let Some(context) = self.context.clone() else { return };
        let Some(rules) = context.rules.clone().filter(|_| !context.target.classes.is_empty())
        else {
            self.set_facts(MoveFacts::default());
            return;
        };
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("refactor-move", move || {
            let cancelled = || cancel.load(Ordering::Relaxed);
            let facts = scan_for_move(
                &context.target,
                context.sources.as_ref(),
                rules.as_ref(),
                &cancelled,
            );
            if !cancelled()
                && tx.send(facts).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.scanning = Some(rx);
    }

    fn relative(&self, path: &Path) -> String {
        let root = self.context.as_ref().map(|c| c.root.as_path());
        let shown = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
        shown.to_string_lossy().into_owned()
    }
}

impl Viewer for MoveView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<MoveData>() else { return };
        let context = data.context.clone();
        let initial = if context.rules.is_some() && !context.target.classes.is_empty() {
            context.target.initial_namespace()
        } else {
            let into =
                context.target.into.strip_prefix(&context.root).unwrap_or(&context.target.into);
            into.to_string_lossy().into_owned()
        };
        self.context = Some(context);
        self.field.set_text(&initial);
        self.field.end(false);
        self.plan = None;
        self.facts = None;
        self.scan();
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.scanning else { return false };
        match rx.try_recv() {
            Ok(facts) => {
                self.scanning = None;
                self.set_facts(facts);
                true
            }
            // Still searching: nothing new to draw (its "searching" stays as it is).
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.scanning = None;
                self.set_facts(MoveFacts::default());
                true
            }
        }
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.type_text(text);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Backspace => self.field.backspace(false),
            ViewerKey::Delete => self.field.delete(),
            ViewerKey::Left => self.field.move_left(false, false),
            ViewerKey::Right => self.field.move_right(false, false),
            ViewerKey::Home => self.field.home(false),
            ViewerKey::End => self.field.end(false),
            ViewerKey::Enter => {
                self.action = self.apply();
                return self.action.is_some();
            }
            ViewerKey::Escape => {
                self.action = Some(ViewerAction::Close);
                return true;
            }
            _ => return false,
        }
        self.replan();
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id == "select_all" {
            self.field.select_all();
            return true;
        }
        false
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // The page's scrollbar; nothing under the keys.
        if self.page.press(p) || !self.page.contains(p) {
            return None;
        }
        if self.list.contains(p) {
            return self.list.click(self.plan.as_mut()?, p);
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        match hit {
            Hit::Apply => return self.apply(),
            Hit::Field => self.field.click(p.x),
            Hit::Close => return Some(ViewerAction::Close),
        }
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        self.list.hover(p) | self.page.hover(p) | changed
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.page.scroll(&mut self.list, dy)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.page.drag(p)
    }

    fn release(&mut self) -> bool {
        self.page.release()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let footer_h = px(40.0);
        // The page above the keys, scrolled when the group is short.
        let (page_area, y0) = self.page.begin(b, footer_h, px(28.0));
        let layer = ctx.scene.push_layer(Some(page_area));
        let width = px(860.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let mut y = y0;
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.0);
        let font = ds.ui_font(0.0);

        // Header: the title and what moves.
        let title = ctx.text.shape_line(&t("refactor.move_title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        let moved =
            self.context.as_ref().map(|c| self.relative(&c.target.from)).unwrap_or_default();
        let bx = x0 + title.width + px(space::MD);
        ds::badge(
            layer,
            ctx.text,
            &ds,
            ds::BadgeSpec {
                label: &moved,
                x: bx,
                mid: y + px(14.0),
                align_right: false,
                tone: Tone::Accent,
            },
        );
        let hovered = self.hover == Some(Hit::Close);
        let cap = ds::keycap(layer, ctx.text, &ds, "esc", x0 + width, y + px(14.0), hovered);
        self.hits.push((cap, Hit::Close));
        y += px(48.0);

        // The field (namespace or folder) and the button.
        let label =
            t(if self.by_namespace() { "refactor.move_namespace" } else { "refactor.move_folder" });
        let shaped = ctx.text.shape_line(&label, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(18.0), &small), p.text_muted);
        y += px(22.0);
        let problem = self.problem();
        let button_w = ds::primary_width(ctx.text, &ds, &[&t("refactor.move_apply")]);
        let field = Rect::new(x0, y, width - button_w - px(space::MD), px(ds::BUTTON_H));
        let style = TextInputStyle {
            font,
            background: p.input_background,
            border: p.input_border,
            border_focused: if matches!(problem, None | Some(Problem::Same)) {
                p.accent
            } else {
                p.danger
            },
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: px(radius::MD),
            border_width: ds.line(),
            padding: px(12.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let placeholder = t(if self.by_namespace() {
            "refactor.global_namespace"
        } else {
            "refactor.project_root"
        });
        self.field.paint(layer, ctx.text, field, true, &placeholder, &style);
        self.hits.push((field, Hit::Field));
        let button = Rect::new(field.max_x() + px(space::MD), y, button_w, px(ds::BUTTON_H));
        let hovered = self.hover == Some(Hit::Apply);
        ds::primary_button(
            layer,
            ctx.text,
            &ds,
            button,
            &t("refactor.move_apply"),
            self.can_apply(),
            hovered,
        );
        self.hits.push((button, Hit::Apply));
        y += px(46.0);

        // Where it goes, or why not; then the summary.
        let (note, color) = match problem {
            Some(Problem::InvalidNamespace) => (t("refactor.invalid_namespace"), p.danger),
            Some(Problem::Exists) => (t("refactor.move_exists"), p.danger),
            Some(Problem::IntoItself) => (t("refactor.move_into_itself"), p.danger),
            Some(Problem::Same) => (t("refactor.move_same"), p.text_muted),
            None => {
                let to = self.destination().map(|d| self.relative(&d)).unwrap_or_default();
                (tr!("refactor.move_to", path = to.as_str()), p.text)
            }
        };
        let note = ds::ellipsize(ctx.text, &note, &small, width);
        let shaped = ctx.text.shape_line(&note, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), color);
        y += px(22.0);
        let summary = match &self.plan {
            Some(plan) if plan.files.is_empty() => t("refactor.move_only"),
            plan => PlanList::summary(plan.as_ref()),
        };
        let shaped = ctx.text.shape_line(&summary, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), p.text_muted);
        y += px(28.0);

        let foot = Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h);
        let (apply, cancel) = (t("refactor.apply_key"), t("refactor.cancel_key"));
        let hints: &[ui_kit::KeyHint<'_>] = &[(&["↵"], &apply), (&["esc"], &cancel)];
        ds::footer_in_card(ctx.scene, ctx.text, &ds, foot, hints, radius::CARD);
        let bottom = foot.min_y() - px(8.0);
        let area = self.page.list_area(x0, width, y, bottom, px(MIN_LIST));
        self.page.paint_bar(ctx.scene, p, ctx.scale);
        let Some(plan) = &self.plan else { return };
        let root = self.context.as_ref().map(|c| c.root.clone());
        self.list.paint(ctx.scene, ctx.text, &ds, plan, root.as_deref(), area);
    }
}

#[cfg(test)]
#[path = "../tests/unit/move_view.rs"]
mod tests;
