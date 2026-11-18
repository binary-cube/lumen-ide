//! Find Usages (a tab): every place the symbol at the cursor is used, file by file — found
//! like a rename's places (the language's navigation decides; unsure places marked) —
//! each line shown with the name highlighted; a click opens it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use refactor::application::{RenameContext, plan_rename};
use refactor::{Certainty, Plan};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{Viewer, ViewerAction, ViewerKey, ViewerPaint};
use ui_kit::draw_text;

use crate::page::{MIN_LIST, PageScroll};
use crate::plan_list::PlanList;

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-usages";

/// What the view is opened with.
#[derive(Debug, Clone)]
pub struct UsagesData {
    pub context: RenameContext,
}

#[derive(Default)]
pub struct UsagesView {
    context: Option<RenameContext>,
    plan: Option<Plan>,
    finding: Option<Receiver<Plan>>,
    cancel: Arc<AtomicBool>,
    list: PlanList,
    page: PageScroll,
    action: Option<ViewerAction>,
    close: Rect,
    close_hovered: bool,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for UsagesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsagesView").finish_non_exhaustive()
    }
}

impl Drop for UsagesView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl UsagesView {
    /// The places found (for tests and the app).
    pub fn usages(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Sets what was found (from the background search, or directly in tests).
    pub fn set_usages(&mut self, mut plan: Plan) {
        // A rename's file renames aren't usages.
        plan.renames.clear();
        self.plan = Some(plan);
    }

    /// "12 usages in 4 files · 2 unsure".
    fn summary(&self) -> String {
        let Some(plan) = &self.plan else { return t("refactor.searching") };
        let count: usize = plan.files.iter().map(|f| f.occurrences.len()).sum();
        if count == 0 {
            return t("refactor.no_usages");
        }
        let unsure = plan
            .files
            .iter()
            .flat_map(|f| &f.occurrences)
            .filter(|o| o.certainty == Certainty::Uncertain)
            .count();
        let found = tr!("refactor.usages_found", count = count, files = plan.files.len());
        if unsure == 0 {
            found
        } else {
            format!("{found} · {}", tr!("refactor.unsure_usages", count = unsure))
        }
    }

    fn search(&mut self) {
        let Some(context) = self.context.clone() else { return };
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("find-usages", move || {
            let cancelled = || cancel.load(Ordering::Relaxed);
            // The same search as a rename's places: every occurrence, resolved.
            let plan = plan_rename(
                &context.target,
                context.sources.as_ref(),
                context.resolver.as_ref(),
                &cancelled,
            );
            if !cancelled()
                && tx.send(plan).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.finding = Some(rx);
    }
}

impl Viewer for UsagesView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<UsagesData>() else { return };
        self.context = Some(data.context.clone());
        self.plan = None;
        self.list.read_only = true;
        self.search();
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.finding else { return false };
        match rx.try_recv() {
            Ok(plan) => {
                self.finding = None;
                self.set_usages(plan);
                true
            }
            // Still searching: nothing new to draw (its "searching" stays as it is).
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.finding = None;
                self.plan = Some(Plan::default());
                true
            }
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if key == ViewerKey::Escape {
            self.action = Some(ViewerAction::Close);
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
        if self.close.contains(p) {
            return Some(ViewerAction::Close);
        }
        self.list.click(self.plan.as_mut()?, p)
    }

    fn hover(&mut self, p: Point) -> bool {
        let close = self.close.contains(p);
        let changed = close != self.close_hovered;
        self.close_hovered = close;
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
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        // Rounded like the editor's card it fills (its bottom corners show).
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let width = px(920.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let footer_h = px(40.0);
        // The page above the keys, scrolled when the group is short.
        let (page_area, mut y) = self.page.begin(b, footer_h, px(28.0));
        let layer = ctx.scene.push_layer(Some(page_area));
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.0);

        // Header: the title, the symbol, `esc`.
        let title = ctx.text.shape_line(&t("refactor.usages_title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        let name = self.context.as_ref().map_or(String::new(), |c| c.target.name.clone());
        let bx = x0 + title.width + px(12.0);
        ds::badge(
            layer,
            ctx.text,
            &ds,
            ds::BadgeSpec {
                label: &name,
                x: bx,
                mid: y + px(14.0),
                align_right: false,
                tone: Tone::Accent,
            },
        );
        let close = x0 + width;
        self.close =
            ds::keycap(layer, ctx.text, &ds, "esc", close, y + px(14.0), self.close_hovered);
        y += px(48.0);
        let summary = ctx.text.shape_line(&self.summary(), &small);
        draw_text(layer, &summary, x0, ds::optical_baseline(y, px(20.0), &small), p.text_muted);
        y += px(32.0);

        // The keys, at the bottom; the places above them.
        let foot = Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h);
        let close = t("keys.close");
        let hints: &[ui_kit::KeyHint<'_>] = &[(&["esc"], &close)];
        ds::footer_in_card(ctx.scene, ctx.text, &ds, foot, hints, radius::CARD);
        let bottom = foot.min_y() - px(8.0);
        let area = self.page.list_area(x0, width, y, bottom, px(MIN_LIST));
        self.page.paint_bar(ctx.scene, p, ctx.scale);
        let Some(plan) = &self.plan else { return };
        let root = self.context.as_ref().and_then(|c| c.sources.root());
        self.list.paint(ctx.scene, ctx.text, &ds, plan, root, area);
    }
}

#[cfg(test)]
#[path = "../tests/unit/usages_view.rs"]
mod tests;
