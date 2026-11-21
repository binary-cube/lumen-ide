//! A refactoring's places, as the previews list them: "every place", then the renamed
//! files, then each file and its places — each with a checkbox, its line number and the
//! line as it will read (the change highlighted); unsure places marked. Shared by the
//! refactorings' previews. A list longer than its room scrolls (the wheel, through the
//! page) and shows the shared scrollbar in a lane of its own on its right.

use std::path::Path;
use ui_components::ds::Pen;

use i18n::{t, tr};
use kernel::{Point, Rect};
use refactor::{Certainty, Plan};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::control::{Control, NOWHERE, Press};
use ui_components::ds::{self, Check, Ctx, Tone, radius, space};
use ui_components::{EdgeScrollbar, Scrollbar, ViewerAction};
use ui_kit::draw_text;

/// A row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A file renamed with the symbol.
    Rename(usize),
    /// A file's header: its name and how many places.
    File(usize),
    /// A place in a file.
    Place(usize, usize),
}

/// The list with the plan it shows, as a control: a press on a row toggles it or shows its
/// line (that action kept in `clicked` for the view).
pub struct Places<'a> {
    pub list: &'a mut PlanList,
    /// Nothing found yet: a press on the list does nothing.
    pub plan: Option<&'a mut Plan>,
    pub clicked: &'a mut Option<ViewerAction>,
}

impl Control for Places<'_> {
    fn hit(&self, p: Point) -> bool {
        self.list.contains(p) || self.list.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        // Its scrollbar first: the thumb held, or the list jumped there.
        if let Some(offset) = self.list.bar.press(p) {
            self.list.scroll = offset.clamp(0.0, self.list.max_scroll);
            return Press::Held;
        }
        if let Some(plan) = self.plan.as_deref_mut() {
            *self.clicked = self.list.click(plan, p);
        }
        Press::Took
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.list.bar.drag(p) else { return false };
        let offset = offset.clamp(0.0, self.list.max_scroll);
        let changed = offset != self.list.scroll;
        self.list.scroll = offset;
        changed
    }

    fn release(&mut self) -> bool {
        self.list.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let p = p.unwrap_or(NOWHERE);
        // Both asked: the bar widens while a row lights.
        let bar = self.list.bar.hover(p);
        self.list.hover(p) | bar
    }
}

/// What a click on the list hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    All,
    Check(Row),
    /// A place's line: shown in the editor.
    Show(usize, usize),
}

#[derive(Debug, Default)]
pub struct PlanList {
    /// Only shows the places (Find Usages): no checkboxes, a click goes to the line.
    pub read_only: bool,
    scroll: f32,
    /// How far it scrolls (from the last paint).
    max_scroll: f32,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    /// The shared scrollbar, on the list's right while it scrolls.
    bar: EdgeScrollbar,
}

/// The room between the rows and the scrollbar's lane (logical pixels).
const BAR_GAP: f32 = 4.0;
/// A place's line under the mouse: its fill's alpha of the text.
const PLACE_HOVER: f32 = 0.04;

impl PlanList {
    /// "3 places in 2 files · 1 unsure", or that it's still searching.
    pub fn summary(plan: Option<&Plan>) -> String {
        let Some(plan) = plan else { return t("refactor.searching") };
        let total: usize = plan.files.iter().map(|f| f.occurrences.len()).sum();
        if total == 0 {
            return t("refactor.no_places");
        }
        let unsure = plan
            .files
            .iter()
            .flat_map(|f| &f.occurrences)
            .filter(|o| o.certainty == Certainty::Uncertain)
            .count();
        let (places, files) = plan.chosen();
        let chosen = tr!("refactor.chosen", places = places, files = files);
        if unsure == 0 {
            chosen
        } else {
            format!("{chosen} · {}", tr!("refactor.unsure", count = unsure))
        }
    }

    fn rows(plan: &Plan) -> Vec<Row> {
        let mut rows: Vec<Row> = (0..plan.renames.len()).map(Row::Rename).collect();
        for (i, file) in plan.files.iter().enumerate() {
            rows.push(Row::File(i));
            rows.extend((0..file.occurrences.len()).map(|j| Row::Place(i, j)));
        }
        rows
    }

    /// Chooses / leaves out a row (a place, a file's places, a file rename).
    pub fn toggle(plan: &mut Plan, row: Row) {
        match row {
            Row::Rename(i) => {
                if let Some(r) = plan.renames.get_mut(i) {
                    r.chosen = !r.chosen;
                }
            }
            Row::File(i) => {
                let all = plan.files.get(i).is_some_and(|f| f.occurrences.iter().all(|o| o.chosen));
                plan.choose_file(i, !all);
            }
            Row::Place(i, j) => {
                if let Some(o) = plan.files.get_mut(i).and_then(|f| f.occurrences.get_mut(j)) {
                    o.chosen = !o.chosen;
                }
            }
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        self.hits.iter().any(|(r, _)| r.contains(p))
    }

    /// A click: a checkbox toggled (`None` back), or a place's line to show.
    pub fn click(&mut self, plan: &mut Plan, p: Point) -> Option<ViewerAction> {
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        match hit {
            Hit::All => {
                let (chosen, _) = plan.chosen();
                let total: usize = plan.files.iter().map(|f| f.occurrences.len()).sum();
                plan.choose_all(chosen < total);
            }
            Hit::Check(row) => Self::toggle(plan, row),
            Hit::Show(i, j) => {
                let file = plan.files.get(i)?;
                let line = file.occurrences.get(j)?.line;
                return Some(ViewerAction::OpenAt { path: file.path.clone(), line });
            }
        }
        None
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn scroll(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    /// How far it's scrolled.
    pub fn offset(&self) -> f32 {
        self.scroll
    }

    /// Paints the list in `area` (the "every place" row at its top, then the rows,
    /// scrolled); `root`: paths are shown relative to it.
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        plan: &Plan,
        root: Option<&Path>,
        area: Rect,
    ) {
        self.hits.clear();
        let px = |v: f32| ds.px(v);
        let (x0, width) = (area.min_x(), area.size.width);
        let row_h = px(30.0);
        let mut y = area.min_y();
        let layer = scene.push_layer(Some(area));
        if !plan.files.is_empty() && !self.read_only {
            self.paint_all(Pen { layer, text, ds }, plan, Rect::new(x0, y, width, row_h));
            y += row_h + px(4.0);
        }
        ds::divider(layer, ds, x0, x0 + width, y);
        y += px(6.0);
        let list = Rect::new(x0, y, width, (area.max_y() - y).max(0.0));
        let rows = Self::rows(plan);
        let content = rows.len() as f32 * row_h;
        self.max_scroll = (content - list.size.height).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        // While it scrolls, the rows leave the bar a lane of its own.
        let lane = if self.max_scroll > 0.0 {
            Scrollbar::track_thickness(ds.scale) + px(BAR_GAP)
        } else {
            0.0
        };
        let layer = scene.push_layer(Some(list));
        let lines = Lines {
            x0,
            width: (width - lane).max(0.0),
            row_h,
            list,
            font: ds.ui_font(0.0),
            small: ds.ui_font(-1.0),
            code: ds.code_font(-1.0),
        };
        for (k, row) in rows.iter().enumerate() {
            let top = list.min_y() + k as f32 * row_h - self.scroll;
            if top + row_h < list.min_y() || top > list.max_y() {
                continue;
            }
            let pen = Pen { layer, text, ds };
            match *row {
                Row::Rename(i) => self.paint_rename(pen, &lines, plan, root, i, top),
                Row::File(i) => self.paint_file(pen, &lines, plan, root, i, top),
                Row::Place(i, j) => self.paint_place(pen, &lines, plan, (i, j), top),
            }
        }
        self.bar.paint(scene, ds.palette, ds.scale, list, content, self.scroll);
    }

    /// The "every place" row, in `row`: a checkbox for all of them.
    fn paint_all(&mut self, pen: Pen<'_, '_>, plan: &Plan, row: Rect) {
        let Pen { layer, text, ds } = pen;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(0.0);
        let (x0, y, row_h) = (row.min_x(), row.min_y(), row.size.height);
        let total: usize = plan.files.iter().map(|f| f.occurrences.len()).sum();
        let mid = y + row_h / 2.0;
        let state = Check::of(plan.chosen().0, total);
        ds::checkbox(layer, text, ds, x0 + px(6.0), mid, state, true);
        let label = text.shape_line(&t("refactor.all_places"), &font);
        draw_text(
            layer,
            &label,
            x0 + px(32.0),
            ds::optical_baseline(y, row_h, &font),
            ds.palette.text_muted,
        );
        self.hits.push((row, Hit::All));
    }

    /// A file renamed with the symbol, at `top`: in the same folder a rename, else a move
    /// (shown from the project).
    fn paint_rename(
        &mut self,
        pen: Pen<'_, '_>,
        lines: &Lines,
        plan: &Plan,
        root: Option<&Path>,
        i: usize,
        top: f32,
    ) {
        let Pen { layer, text, ds } = pen;
        let px = |v: f32| ds.px(v);
        let Lines { x0, width, row_h, list, ref font, .. } = *lines;
        let mid = top + row_h / 2.0;
        let visible = Rect::new(x0, top, width, row_h).intersect(&list).unwrap_or_default();
        let r = &plan.renames[i];
        let state = if r.chosen { Check::On } else { Check::Off };
        ds::checkbox(layer, text, ds, x0 + px(6.0), mid, state, true);
        let label = if r.from.parent() == r.to.parent() {
            tr!(
                "refactor.rename_file",
                from = kernel::paths::file_name(&r.from).as_str(),
                to = kernel::paths::file_name(&r.to).as_str()
            )
        } else {
            let shown = |p: &Path| {
                root.and_then(|r| p.strip_prefix(r).ok())
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned()
            };
            tr!("refactor.move_file", from = shown(&r.from).as_str(), to = shown(&r.to).as_str())
        };
        let label = ds::ellipsize(text, &label, font, width - px(40.0));
        let shaped = text.shape_line(&label, font);
        let baseline = ds::optical_baseline(top, row_h, font);
        draw_text(layer, &shaped, x0 + px(32.0), baseline, ds.palette.text);
        self.hits.push((visible, Hit::Check(Row::Rename(i))));
    }

    /// A file's header at `top`: its checkbox, name and folder, how many places.
    fn paint_file(
        &mut self,
        pen: Pen<'_, '_>,
        lines: &Lines,
        plan: &Plan,
        root: Option<&Path>,
        i: usize,
        top: f32,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Lines { x0, width, row_h, list, ref font, ref small, .. } = *lines;
        let mid = top + row_h / 2.0;
        let visible = Rect::new(x0, top, width, row_h).intersect(&list).unwrap_or_default();
        let file = &plan.files[i];
        let chosen = file.occurrences.iter().filter(|o| o.chosen).count();
        let state = Check::of(chosen, file.occurrences.len());
        if !self.read_only {
            ds::checkbox(layer, text, ds, x0 + px(6.0), mid, state, true);
        }
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let name = text.shape_line(&kernel::paths::file_name(&file.path), &strong);
        let nx = x0 + if self.read_only { px(8.0) } else { px(32.0) };
        draw_text(layer, &name, nx, ds::optical_baseline(top, row_h, &strong), p.text);
        let count = file.occurrences.len().to_string();
        let right = x0 + width - px(4.0);
        let badge = ds::badge(
            layer,
            text,
            ds,
            ds::BadgeSpec { label: &count, x: right, mid, align_right: true, tone: Tone::Neutral },
        );
        let dir = relative_dir(&file.path, root);
        let dx = nx + name.width + px(space::MD);
        let room = badge.min_x() - px(space::MD) - dx;
        let dir = ds::ellipsize_middle(text, &dir, small, room);
        let d = text.shape_line(&dir, small);
        draw_text(layer, &d, dx, ds::optical_baseline(top, row_h, small), p.text_muted);
        if !self.read_only {
            self.hits.push((visible, Hit::Check(Row::File(i))));
        }
    }

    /// A place at `top`: its checkbox, line number and the line as it will read (unsure
    /// ones marked).
    fn paint_place(
        &mut self,
        pen: Pen<'_, '_>,
        lines: &Lines,
        plan: &Plan,
        (i, j): (usize, usize),
        top: f32,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Lines { x0, width, row_h, list, ref code, .. } = *lines;
        let mid = top + row_h / 2.0;
        let o = &plan.files[i].occurrences[j];
        let indent = x0 + px(28.0);
        let shade = Rect::new(
            indent + px(26.0),
            top + px(2.0),
            x0 + width - indent - px(26.0),
            row_h - px(4.0),
        );
        ds::RowHighlight::new(false, self.hover == Some(Hit::Show(i, j)))
            .radius(radius::SM)
            .hover(ds::HoverFill::Faint(PLACE_HOVER))
            .paint(layer, ds, shade);
        let check = if self.read_only {
            Rect::new(indent - px(10.0), mid, 0.0, 0.0)
        } else {
            let state = if o.chosen { Check::On } else { Check::Off };
            ds::checkbox(layer, text, ds, indent + px(6.0), mid, state, true)
        };
        let number = text.shape_line(&(o.line + 1).to_string(), code);
        let lx = check.max_x() + px(10.0);
        let gutter = px(40.0);
        let baseline = ds::optical_baseline(top, row_h, code);
        draw_text(layer, &number, lx + gutter - number.width - px(10.0), baseline, p.text_muted);
        let tx = lx + gutter;
        let right = if o.certainty == Certainty::Uncertain {
            let label = t("refactor.unsure_badge");
            let unsure = ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec {
                    label: &label,
                    x: x0 + width - px(4.0),
                    mid,
                    align_right: true,
                    tone: Tone::Neutral,
                },
            );
            unsure.min_x() - px(space::SM)
        } else {
            x0 + width - px(4.0)
        };
        paint_line(
            Pen { layer, text, ds },
            code,
            (tx, top, row_h, right - tx),
            &o.line_text,
            o.in_line.clone(),
            &o.replacement,
        );
        let line = Rect::new(lx, top, x0 + width - lx, row_h);
        self.hits.push((line.intersect(&list).unwrap_or_default(), Hit::Show(i, j)));
        self.hits.push((check.intersect(&list).unwrap_or_default(), Hit::Check(Row::Place(i, j))));
    }
}

/// Where the list's rows go: their left edge, width and height, the list (their hit areas
/// are clipped to it), and the fonts.
struct Lines {
    x0: f32,
    width: f32,
    row_h: f32,
    list: Rect,
    font: Font,
    small: Font,
    code: Font,
}

/// A place's line as it will read: the text around it, the new text highlighted in place
/// of the old; leading space dropped, cut to `room`.
fn paint_line(
    pen: Pen<'_, '_>,
    font: &Font,
    (x, top, height, room): (f32, f32, f32, f32),
    line: &str,
    at: std::ops::Range<usize>,
    replacement: &str,
) {
    let Pen { layer, text, ds } = pen;
    let p = ds.palette;
    let at = at.start.min(line.len())..at.end.min(line.len());
    let start = line.len() - line.trim_start().len();
    let start = start.min(at.start);
    let before = &line[start..at.start];
    let after = &line[at.end..];
    let baseline = ds::optical_baseline(top, height, font);
    let shaped_before = text.shape_line(before, font);
    // Long lines keep the change in view: the start is cut.
    let before = if shaped_before.width > room * 0.45 {
        ds::ellipsize_start(text, before, font, room * 0.45)
    } else {
        before.to_owned()
    };
    let shaped = text.shape_line(&before, font);
    draw_text(layer, &shaped, x, baseline, p.text_muted);
    let nx = x + shaped.width;
    // The new text on one line (a declaration's list may span several).
    let flat: String = replacement.split_whitespace().collect::<Vec<_>>().join(" ");
    let flat = ds::ellipsize(text, &flat, font, (x + room - nx).max(0.0));
    let name = text.shape_line(&flat, font);
    let pad = ds.px(2.0);
    let mark = Rect::new(nx - pad, top + ds.px(5.0), name.width + pad * 2.0, height - ds.px(10.0));
    layer.quad(Quad::filled(mark, p.accent.with_alpha(0.18)).rounded(ds.px(4.0)));
    draw_text(layer, &name, nx, baseline, p.text);
    let ax = nx + name.width;
    let after = ds::ellipsize(text, after, font, (x + room - ax).max(0.0));
    let shaped = text.shape_line(&after, font);
    draw_text(layer, &shaped, ax, baseline, p.text_muted);
}

/// The file's folder, relative to the project when it's inside it.
fn relative_dir(path: &Path, root: Option<&Path>) -> String {
    let dir = path.parent().unwrap_or(Path::new(""));
    let shown = root.and_then(|r| dir.strip_prefix(r).ok()).unwrap_or(dir);
    shown.to_string_lossy().into_owned()
}

#[cfg(test)]
#[path = "../tests/unit/plan_list.rs"]
mod tests;
