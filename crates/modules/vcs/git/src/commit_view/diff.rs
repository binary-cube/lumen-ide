//! The selected file's changes: read in the background, painted hunk by hunk.

use super::*;

/// A row of the changes' pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffRow {
    Hunk(usize),
    Line(usize, usize),
}

/// The selected file's changes, read in the background.
#[derive(Debug)]
pub(super) struct FileDiff {
    pub(super) path: String,
    pub(super) staged: bool,
    pub(super) hunks: Vec<DiffHunk>,
    pub(super) rows: Vec<DiffRow>,
    /// Each hunk's lines, colored (empty: in the code's own color).
    pub(super) colors: Vec<Vec<Vec<syntax::Piece>>>,
    /// Its hunks can be moved one by one (a new, deleted or renamed file goes whole).
    pub(super) partial: bool,
    /// Which reading of changes it is (never twice: what's kept for it, its wrapped rows,
    /// is never taken for another's).
    pub(super) id: u64,
}

/// The changes' readings counted (each [`FileDiff`]'s id).
static READINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A new reading's id.
pub(super) fn reading_id() -> u64 {
    READINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl CommitView {
    /// The selected file's changes in `area`, hunk by hunk.
    pub(super) fn paint_diff(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        card: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let p = ds.palette;
        let layer = scene.push_layer(Some(card));
        let title = self.selected.as_ref().map_or_else(|| t("git.changes_tab"), |(f, _)| f.clone());
        let area = paint_card(layer, text, ds, card, (&title, px(12.0)));
        // Above the card's rounded corners: no row reaches them.
        let area = Rect::new(
            area.min_x(),
            area.min_y(),
            area.size.width,
            (area.size.height - px(10.0)).max(0.0),
        );
        self.panes[DIFF] = area;
        let Some(diff) = self.diff.take() else {
            ds::empty_state(layer, text, ds, area, &t("git.select_file"), None);
            return;
        };
        if diff.rows.is_empty() {
            ds::empty_state(layer, text, ds, area, &t("git.no_diff"), None);
            self.diff = Some(diff);
            return;
        }
        let row_h = px(20.0);
        let code = ds.code_font(-1.0);
        let small = ds.ui_font(-1.5);
        let number_w = px(38.0);
        let (mut widest, right_edge) = (0.0_f32, area.max_x() - px(6.0));
        // Lines wrapping: at the code's width (a column kept for the wrap's mark).
        let column = text.shape_line("0", &code).width;
        let code_from = area.min_x() + px(6.0) + 2.0 * number_w + px(14.0);
        let columns = if self.wrap {
            (((right_edge - code_from) / column.max(1.0)).floor() as usize).saturating_sub(1)
        } else {
            0
        };
        self.code.wrap.refresh(&diff, columns);
        if self.code.wrap.on() {
            self.diff_x = 0.0;
        }
        let wrap = &self.code.wrap;
        let content = wrap.total(diff.rows.len()) as f32 * row_h;
        self.scrolls[DIFF] = self.scrolls[DIFF].min((content - area.size.height).max(0.0));
        let scroll = self.scrolls[DIFF];
        let (first, _) = wrap.at((scroll / row_h) as u32);
        let bottom = scroll + area.size.height;
        let count =
            (first..diff.rows.len()).take_while(|&r| wrap.first(r) as f32 * row_h < bottom).count()
                + 1;
        let rows = scene.push_layer(Some(area));
        let mut bands = Vec::new();
        for (at, row) in diff.rows.iter().enumerate().skip(first).take(count) {
            let y = area.min_y() + self.code.wrap.first(at) as f32 * row_h - scroll;
            let height = self.code.wrap.rows(at) as f32 * row_h;
            let band = Rect::new(area.min_x(), y, area.size.width, height);
            match *row {
                DiffRow::Hunk(h) => {
                    self.paint_hunk_header(rows, text, icons, ds, (band, &small), (&diff, h))
                }
                DiffRow::Line(h, l) => {
                    let line = &diff.hunks[h].lines[l];
                    let (tint, sign) = match line.kind {
                        LineKind::Context => (None, " "),
                        LineKind::Added => (Some(p.vcs_added), "+"),
                        LineKind::Removed => (Some(p.danger), "-"),
                    };
                    if let Some(tint) = tint {
                        let chosen = self.lines.contains(&(h, l));
                        rows.quad(Quad::filled(
                            band,
                            tint.with_alpha(if chosen { 0.34 } else { 0.13 }),
                        ));
                        if chosen {
                            let edge =
                                Rect::new(band.min_x(), band.min_y(), px(3.0), band.size.height);
                            rows.quad(Quad::filled(edge, p.accent));
                        }
                        if diff.partial {
                            // The numbers choose the line; the code is for selecting text.
                            let gutter = Rect::new(
                                band.min_x(),
                                band.min_y(),
                                px(6.0) + 2.0 * number_w + px(14.0),
                                band.size.height,
                            );
                            let visible = gutter.intersect(&area).unwrap_or_default();
                            self.hits.push((visible, Hit::Line(h, l)));
                        }
                    }
                    let baseline = ds::optical_baseline(y, row_h, &code);
                    let mut x = area.min_x() + px(6.0);
                    for number in [line.old, line.new] {
                        if let Some(number) = number {
                            let shaped = text.shape_line(&number.to_string(), &code);
                            let right = x + number_w - shaped.width - px(6.0);
                            draw_text(rows, &shaped, right, baseline, p.text_muted);
                        }
                        x += number_w;
                    }
                    let shaped = text.shape_line(sign, &code);
                    draw_text(rows, &shaped, x, baseline, tint.unwrap_or(p.text_muted));
                    let clean = line.text.trim_end_matches(['\n', '\r']);
                    let plain = [(0..clean.len(), None)];
                    let pieces = diff.colors.get(h).and_then(|lines| lines.get(l));
                    let pieces = pieces.filter(|c| !c.is_empty()).map_or(&plain[..], Vec::as_slice);
                    let code_left = x + px(14.0);
                    self.code.place(text_selection::CodeArea {
                        area,
                        left: code_left,
                        row_h,
                        column: text.shape_line("0", &code).width,
                        scroll,
                        sideways: self.diff_x,
                    });
                    let width = clean_columns(line);
                    self.code.bands(at, width, (code_left, right_edge), &mut bands);
                    for rect in &bands {
                        rows.quad(Quad::filled(*rect, p.accent.with_alpha(0.3)));
                    }
                    // Its rows (one, unless it wraps), each its columns' share of the pieces.
                    let parts = self.code.wrap.parts(at, width);
                    let mut pen = code_left - self.diff_x;
                    let mut column_at = 0;
                    let mut part = 0;
                    for (range, capture) in pieces {
                        let Some(piece) = clean.get(range.clone()) else { continue };
                        let drawn = piece.replace('\t', "    ");
                        let color = capture
                            .as_deref()
                            .and_then(|name| p.syntax.as_ref()?.capture(name))
                            .unwrap_or(p.text);
                        let mut rest = drawn.as_str();
                        while !rest.is_empty() {
                            let end = parts.get(part).map_or(usize::MAX, |(_, end)| *end);
                            let room = end.saturating_sub(column_at);
                            let cut = rest.char_indices().nth(room).map_or(rest.len(), |(i, _)| i);
                            let (now, later) = rest.split_at(cut);
                            if !now.is_empty() {
                                let shaped = text.shape_line(now, &code);
                                let line_y = baseline + part as f32 * row_h;
                                ui_kit::draw_text_within(
                                    rows,
                                    &shaped,
                                    pen,
                                    line_y,
                                    color,
                                    (code_left, right_edge),
                                );
                                pen += shaped.width;
                                column_at += now.chars().count();
                            }
                            rest = later;
                            if !rest.is_empty() || (column_at >= end && part + 1 < parts.len()) {
                                if part + 1 >= parts.len() {
                                    break;
                                }
                                // The line goes on below: the wrap's mark, then the next row.
                                let mark = text.shape_line(WRAP_END, &code);
                                let line_y = baseline + part as f32 * row_h;
                                draw_text(
                                    rows,
                                    &mark,
                                    pen + px(4.0),
                                    line_y,
                                    p.text_muted.with_alpha(0.6),
                                );
                                part += 1;
                                pen = code_left;
                            }
                        }
                    }
                    widest = widest.max(pen + self.diff_x - right_edge);
                }
            }
        }
        self.diff_overflow = widest.max(0.0);
        self.diff_x = self.diff_x.min(self.diff_overflow);
        self.bars[DIFF].set_auto_hiding(true);
        self.bars[DIFF].paint(scene, p, ds.scale, area, content, scroll);
        self.diff = Some(diff);
    }

    /// Hunk `h`'s header in `band`: the lines it covers and what can be done with it.
    pub(super) fn paint_hunk_header(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        (band, font): (Rect, &Font),
        (diff, h): (&FileDiff, usize),
    ) {
        let px = |v: f32| ds.px(v);
        let p = ds.palette;
        layer.quad(Quad::filled(band, p.text.with_alpha(0.05)));
        let (from, to) = diff.hunks[h].shown_lines();
        let title =
            tr!("git.hunk_title", number = (h + 1) as i64, from = from as i64, to = to as i64);
        let shaped = text.shape_line(&title, font);
        let baseline = ds::optical_baseline(band.min_y(), band.size.height, font);
        draw_text(layer, &shaped, band.min_x() + px(10.0), baseline, p.text_muted);
        if !diff.partial {
            return;
        }
        let mut right = band.max_x() - px(10.0);
        let size = px(13.0);
        let mut link = |view: &mut Self, label: String, icon: &str, hit: Hit, color: Rgba| {
            let shaped = text.shape_line(&label, font);
            right -= shaped.width;
            draw_text(layer, &shaped, right, baseline, color);
            let icon_x = right - px(5.0) - size;
            if let Some(id) = icons.id(icon) {
                let top = band.min_y() + (band.size.height - size) / 2.0;
                ui_kit::draw_icon(layer, icons, id, Rect::new(icon_x, top, size, size), color);
            }
            let width = shaped.width + size + px(13.0);
            view.hits
                .push((Rect::new(icon_x - px(4.0), band.min_y(), width, band.size.height), hit));
            right = icon_x - px(14.0);
        };
        let some = self.lines.iter().any(|(hunk, _)| *hunk == h);
        if !diff.staged {
            let label = if some { t("git.discard_lines") } else { t("git.discard_hunk") };
            link(self, label, "trash", Hit::DiscardHunk(h), p.danger);
        }
        let label = match (diff.staged, some) {
            (true, true) => t("git.unstage_lines"),
            (true, false) => t("git.unstage_hunk"),
            (false, true) => t("git.stage_lines"),
            (false, false) => t("git.stage_hunk"),
        };
        let icon = if diff.staged { "minus" } else { "plus" };
        link(self, label, icon, Hit::StageHunk(h), p.accent);
    }

    pub(super) fn load_diff(&mut self) {
        let (Some(repo), Some((path, staged))) = (self.repo.clone(), self.selected.clone()) else {
            self.diff = None;
            return;
        };
        let file = self.files.iter().find(|f| f.path == path);
        let state = file.and_then(|f| if staged { f.staged } else { f.unstaged });
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("git-status", move || {
            let _ = tx.send(read_diff(&*repo, path, staged, state));
            if let Some(wake) = waker {
                wake();
            }
        });
        self.diff_rx = Some(rx);
    }

    /// The patch of hunk `i`: its lines chosen, or all of it when none are.
    pub(super) fn hunk_patch(&self, i: usize, reverse: bool) -> Option<String> {
        let diff = self.diff.as_ref()?;
        let hunk = diff.hunks.get(i)?;
        if !self.lines.iter().any(|(h, _)| *h == i) {
            return Some(hunk.patch(&diff.path));
        }
        Some(hunk.patch_lines(&diff.path, |l| self.lines.contains(&(i, l)), reverse))
    }

    /// A click on a changed line: chosen or not, ⇧ a run of its hunk from the last one clicked.
    pub(super) fn pick_line(&mut self, h: usize, l: usize) {
        let mods = std::mem::take(&mut self.mods);
        match self.line_anchor {
            Some((hunk, from)) if mods.shift && hunk == h => {
                let changed = |at: &usize| {
                    self.diff
                        .as_ref()
                        .is_some_and(|d| d.hunks[h].lines[*at].kind != LineKind::Context)
                };
                let run: Vec<usize> = (from.min(l)..=from.max(l)).filter(changed).collect();
                self.lines.extend(run.into_iter().map(|at| (h, at)));
            }
            _ => {
                if !self.lines.remove(&(h, l)) {
                    self.lines.insert((h, l));
                }
                self.line_anchor = Some((h, l));
            }
        }
    }

    /// Hunk `i` moved to the other list.
    pub(super) fn toggle_hunk(&mut self, i: usize) {
        let Some(reverse) = self.diff.as_ref().map(|diff| diff.staged) else { return };
        let Some(patch) = self.hunk_patch(i, reverse) else { return };
        self.act(move |repo| repo.apply_patch(&patch, PatchTarget::Staged, reverse));
    }
}

/// The columns `line` takes when shown.
/// The mark at a wrapped row's end (its line goes on below).
const WRAP_END: &str = "↩";

fn clean_columns(line: &version_control::staging::DiffLine) -> usize {
    text_selection::columns(line.text.trim_end_matches(['\n', '\r']))
}

/// The text of a file on disk: empty if it's gone, `None` if it can't be shown (too large,
/// not text).
pub(super) fn working_text(file: &std::path::Path) -> Option<String> {
    let Ok(meta) = std::fs::metadata(file) else { return Some(String::new()) };
    if usize::try_from(meta.len()).map_or(true, |len| len > MAX_DIFF_BYTES) {
        return None;
    }
    String::from_utf8(std::fs::read(file).ok()?).ok()
}

/// `path`'s changes, staged (from the last commit to the staging area) or not (from there to
/// the file on disk). A file too large or not text has no hunks.
pub(super) fn read_diff(
    repo: &dyn Repository,
    path: String,
    staged: bool,
    state: Option<FileState>,
) -> FileDiff {
    let old = repo.text_at(if staged { &Revision::Head } else { &Revision::Index }, &path);
    let new = if staged {
        Some(repo.text_at(&Revision::Index, &path).unwrap_or_default())
    } else {
        working_text(&repo.root().join(&path))
    };
    let old = old.unwrap_or_default();
    let (found, colors) = match new {
        Some(new) if old.len().max(new.len()) <= MAX_DIFF_BYTES => {
            let found = hunks(&old, &new, CONTEXT);
            let colors = syntax::color_hunks(&path, &old, &new, &found);
            (found, colors)
        }
        _ => (Vec::new(), Vec::new()),
    };
    let mut rows = Vec::new();
    for (h, hunk) in found.iter().enumerate() {
        rows.push(DiffRow::Hunk(h));
        rows.extend((0..hunk.lines.len()).map(|l| DiffRow::Line(h, l)));
    }
    let partial = state == Some(FileState::Modified);
    FileDiff { path, staged, hunks: found, rows, partial, colors, id: reading_id() }
}
