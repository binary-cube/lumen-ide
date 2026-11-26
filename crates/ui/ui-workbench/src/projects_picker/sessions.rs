//! The past sessions in the list (plan 78): each a tree — the session (when, how many
//! windows and files, whether it ended badly, a "Reopen" button), its windows (the project,
//! or the files opened alone), their files. Expanded and collapsed here; collapsed at
//! first (a row each), so the projects below stay near.

use super::paint::{ListArea, paint_text_button};
use super::*;

impl ProjectsPicker {
    /// The past sessions' rows, under their title.
    pub(super) fn push_sessions(&mut self, items: &mut Vec<Item>, sessions: &[HomeSession]) {
        if sessions.is_empty() {
            return;
        }
        items.push(Item::FoldHeader(i18n::t("home.sessions"), Section::Sessions));
        if self.folded.contains(&Section::Sessions) {
            return;
        }
        for session in sessions {
            let files: usize = session.windows.iter().map(|w| w.files.len()).sum();
            let expanded = self.sessions_open.contains(&session.id);
            items.push(Item::Session {
                id: session.id,
                when: session.when.clone(),
                detail: i18n::tr!(
                    "home.session_detail",
                    windows = session.windows.len() as i64,
                    files = files as i64
                ),
                crashed: session.crashed,
                expanded,
            });
            if !expanded {
                continue;
            }
            for (index, window) in session.windows.iter().enumerate() {
                let open = self.session_windows_open.contains(&(session.id, index));
                let expanded = open && !window.files.is_empty();
                items.push(Item::SessionWindow {
                    session: session.id,
                    index,
                    name: window.name.clone(),
                    detail: window.detail.clone(),
                    project: window.project,
                    path: window.path.clone(),
                    count: window.files.len(),
                    expanded,
                });
                if expanded {
                    items.extend(window.files.iter().cloned().map(Item::SessionFile));
                }
            }
        }
    }

    /// A past session's row or one of its windows expanded or collapsed, or a section
    /// folded or opened (`false`: `action` isn't one of these).
    pub fn toggle_fold(&mut self, action: &PickerAction) -> bool {
        match *action {
            PickerAction::ToggleSession(id) => {
                if !self.sessions_open.remove(&id) {
                    self.sessions_open.insert(id);
                }
            }
            PickerAction::ToggleSessionWindow(id, index) => {
                if !self.session_windows_open.remove(&(id, index)) {
                    self.session_windows_open.insert((id, index));
                }
            }
            PickerAction::ToggleSection(section) => {
                if !self.folded.remove(&section) {
                    self.folded.insert(section);
                }
            }
            _ => return false,
        }
        true
    }

    /// ← / → on the selected past session or one of its windows (with files): collapsed or
    /// expanded. `false`: neither is selected (the key is someone else's).
    pub fn fold_selected_session(&mut self, collapse: bool) -> bool {
        let (toggle, open) = match self.items.get(self.selected) {
            Some(Item::Session { id, expanded, .. }) => {
                (PickerAction::ToggleSession(*id), *expanded)
            }
            Some(Item::SessionWindow { session, index, count, expanded, .. }) if *count > 0 => {
                (PickerAction::ToggleSessionWindow(*session, *index), *expanded)
            }
            _ => return false,
        };
        if open == collapse {
            self.toggle_fold(&toggle);
        }
        true
    }

    /// The session whose "Reopen" button is at `p`.
    pub(super) fn restore_at(&self, p: Point) -> Option<u128> {
        self.restore_buttons
            .iter()
            .find(|(r, _)| r.contains(p) && self.list.contains(p))
            .map(|(_, id)| *id)
    }
}

/// What a session's row shows.
pub(super) struct SessionRow<'a> {
    pub when: &'a str,
    pub detail: &'a str,
    pub crashed: bool,
    pub expanded: bool,
}

/// What a session window's row shows.
pub(super) struct WindowRow<'a> {
    pub name: &'a str,
    pub detail: &'a str,
    pub project: bool,
    pub count: usize,
    pub expanded: bool,
}

/// Where an expandable row's chevron ends (logical pixels from the row's left, with a margin
/// to hit it): a click before it folds, after it acts.
pub(super) const CHEVRON_END: f32 = 30.0;

/// The chevron of an expandable row `r`, at its left.
fn paint_chevron(list: &mut ListArea<'_, '_>, r: Rect, expanded: bool) {
    let ctx = list.ctx;
    let s = ctx.px(12.0);
    let mid = r.min_y() + r.size.height / 2.0;
    let ir = Rect::new(r.min_x() + ctx.px(12.0), mid - s / 2.0, s, s);
    let color = ctx.fade(ctx.palette.text_muted);
    ui_components::tree::chevron(list.layer, list.icons, ir, expanded, color);
}

/// An icon of 16 points at `x`, in the middle of row `r`.
fn paint_row_icon(list: &mut ListArea<'_, '_>, r: Rect, x: f32, name: &str, color: Rgba) {
    let ctx = list.ctx;
    if let Some(id) = list.icons.id(name) {
        let s = ctx.px(16.0);
        let ir = Rect::new(x, r.min_y() + (r.size.height - s) / 2.0, s, s);
        draw_icon(list.layer, list.icons, id, ir, ctx.fade(color));
    }
}

/// A session's row `r`: the chevron, the clock, when (and a mark if it ended badly), how
/// many windows and files, the "Reopen" button; returns the button.
pub(super) fn paint_session(
    list: &mut ListArea<'_, '_>,
    r: Rect,
    row: &SessionRow<'_>,
    hovered: bool,
) -> Rect {
    let ctx = list.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    paint_chevron(list, r, row.expanded);
    paint_row_icon(list, r, r.min_x() + px(30.0), "clock", p.accent);
    let label = i18n::t("home.session_reopen");
    let mid = r.min_y() + r.size.height / 2.0;
    let button =
        paint_text_button(list, &label, "rotate-ccw", r.max_x() - px(8.0), mid - px(12.0), hovered);
    let x = r.min_x() + px(54.0);
    let room = button.min_x() - px(10.0) - x;
    let font = list.font.clone().with_weight(Font::SEMIBOLD);
    let when = ds::ellipsize(list.text, row.when, &font, room);
    let shaped = list.text.shape_line(&when, &font);
    draw_text(list.layer, &shaped, x, r.min_y() + px(19.0), ctx.fade(p.text));
    let chip_x = x + shaped.width + px(10.0);
    if row.crashed && button.min_x() - chip_x > px(60.0) {
        let label = i18n::t("home.session_crashed");
        let label = ds::ellipsize(
            list.text,
            &label,
            &ctx.ui_font(-1.5),
            button.min_x() - chip_x - px(20.0),
        );
        ds::badge(
            list.layer,
            list.text,
            ctx,
            ds::BadgeSpec {
                label: &label,
                x: chip_x,
                mid: r.min_y() + px(14.0),
                align_right: false,
                tone: ds::Tone::Warning,
            },
        );
    }
    let detail = ds::ellipsize(list.text, row.detail, &list.small, room);
    let shaped = list.text.shape_line(&detail, &list.small);
    draw_text(list.layer, &shaped, x, r.min_y() + px(35.0), ctx.fade(p.text_muted));
    button
}

/// A session window's row `r`: the chevron (when it has files), the project's folder or a
/// window for files alone, the name, the path dimmed, how many files.
pub(super) fn paint_window(list: &mut ListArea<'_, '_>, r: Rect, row: &WindowRow<'_>) {
    let ctx = list.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    if row.count > 0 {
        paint_chevron(list, r, row.expanded);
    }
    let icon = if row.project { "folder" } else { "app-window" };
    paint_row_icon(list, r, r.min_x() + px(30.0), icon, p.text_muted);
    let count = list.text.shape_line(&row.count.to_string(), &list.small);
    let count_x = r.max_x() - px(14.0) - count.width;
    let base = ds::optical_baseline(r.min_y(), r.size.height, &list.small);
    if row.count > 0 {
        draw_text(list.layer, &count, count_x, base, ctx.fade(p.text_muted));
    }
    let x = r.min_x() + px(54.0);
    let room = count_x - px(10.0) - x;
    let name = ds::ellipsize(list.text, row.name, &list.font, room);
    let shaped = list.text.shape_line(&name, &list.font);
    let name_base = ds::optical_baseline(r.min_y(), r.size.height, &list.font);
    draw_text(list.layer, &shaped, x, name_base, ctx.fade(p.text));
    let dx = x + shaped.width + px(10.0);
    if !row.detail.is_empty() && count_x - px(10.0) - dx > px(30.0) {
        let detail = ds::ellipsize(list.text, row.detail, &list.small, count_x - px(10.0) - dx);
        let shaped = list.text.shape_line(&detail, &list.small);
        draw_text(list.layer, &shaped, dx, base, ctx.fade(p.text_muted));
    }
}
