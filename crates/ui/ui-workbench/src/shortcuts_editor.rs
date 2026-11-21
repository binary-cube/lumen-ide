//! Settings → Shortcuts: every command with its keys (searched by name or by keys), where
//! it works, whether the user changed it. A click opens a command: its keys (each removed
//! with ×), "Add shortcut" (the next keys pressed are recorded), "Back to the preset".
//! Keys another command has in the same place: remove them from the
//! others, leave them to all (a choice when pressed), or give up.

use std::sync::Arc;

use command_model::KeyChord;
use kernel::{Point, Rect};
use render_api::{Layer, Quad};
use text_layout::TextSystem;
use ui_components::ds::{self, Ctx};
use ui_kit::draw_text;

/// A command in the list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BindingRow {
    /// Its id.
    pub command: String,
    pub label: String,
    /// Its menu, or the view it works in (translated).
    pub group: String,
    /// Where its keys work (`""`: everywhere; `editor`, a language's editor, a view's id).
    pub place: String,
    /// Text is typed where its keys work (everywhere, an editor): a plain character there
    /// would take the typing.
    pub typing: bool,
    /// Its shortcuts as written (`CmdOrCtrl+K`) and as the system shows them (`⌘K`).
    pub shortcuts: Vec<String>,
    pub shown: Vec<String>,
    /// The user changed them (over the preset's).
    pub changed: bool,
}

/// The commands' new shortcuts: each command's whole list, or `None` (back to the preset's).
pub type ShortcutChanges = Vec<(String, Option<Vec<String>>)>;

#[derive(Debug, Clone, PartialEq)]
enum Hit {
    Row(String),
    /// A command's `n`th shortcut, removed.
    Remove(String, usize),
    Add(String),
    Reset(String),
    RemoveOthers,
    Leave,
    Cancel,
}

/// Keys recorded that others have in the same place: what to do is asked.
#[derive(Debug, Clone)]
struct Conflict {
    command: String,
    keys: String,
    shown: String,
    others: Vec<String>,
}

/// The editor's state.
#[derive(Debug, Default)]
pub struct ShortcutsEditor {
    /// The list's search (a part of the settings' controls: its focus is theirs).
    pub filter: ui_components::search_field::SearchField,
    open: Option<String>,
    recording: bool,
    conflict: Option<Conflict>,
    rows: Arc<Vec<BindingRow>>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
}

/// Whether keys in places `a` and `b` can be pressed at once.
fn overlap(a: &str, b: &str) -> bool {
    a.is_empty() || b.is_empty() || a == b
}

/// The chord of a written shortcut on this platform.
fn chord(spec: &str) -> Option<KeyChord> {
    KeyChord::parse(spec).ok().map(|c| c.for_platform(cfg!(target_os = "macos")))
}

impl ShortcutsEditor {
    /// The next keys pressed are a shortcut being recorded.
    pub fn recording(&self) -> bool {
        self.recording
    }

    /// Opens a command, recording its next keys (as a click on it, then on "Add").
    pub fn open_recording(&mut self, command: &str) {
        self.open = Some(command.to_owned());
        self.recording = true;
        self.conflict = None;
    }

    /// Esc: stops recording, gives up a conflict. `false` if neither was.
    pub fn cancel(&mut self) -> bool {
        if self.recording || self.conflict.is_some() {
            self.recording = false;
            self.conflict = None;
            return true;
        }
        false
    }

    /// Keys pressed while recording (`spec`, as written): added to the open command, or, when
    /// others have them in the same place, the question asked.
    pub fn record(&mut self, spec: &str, shown: &str) -> Option<ShortcutChanges> {
        self.recording = false;
        let command = self.open.clone()?;
        let row = self.rows.iter().find(|r| r.command == command)?.clone();
        let pressed = chord(spec)?;
        // A plain character where text is typed would take the typing: not taken, still
        // recording.
        let bare = !(pressed.primary || pressed.alt || pressed.control)
            && matches!(pressed.key, command_model::ChordKey::Char(_));
        if row.typing && bare {
            self.recording = true;
            return None;
        }
        if row.shortcuts.iter().any(|s| chord(s).as_ref() == Some(&pressed)) {
            return None;
        }
        let others: Vec<String> = self
            .rows
            .iter()
            .filter(|r| r.command != command && overlap(&r.place, &row.place))
            .filter(|r| r.shortcuts.iter().any(|s| chord(s).as_ref() == Some(&pressed)))
            .map(|r| r.command.clone())
            .collect();
        if others.is_empty() {
            return Some(vec![(command, Some(with(&row.shortcuts, spec)))]);
        }
        self.conflict =
            Some(Conflict { command, keys: spec.to_owned(), shown: shown.to_owned(), others });
        None
    }

    /// A click: opens a command, removes / adds / resets its keys, answers a conflict.
    pub fn click(&mut self, p: Point) -> Option<ShortcutChanges> {
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        let row = |command: &str| self.rows.iter().find(|r| r.command == command).cloned();
        match hit? {
            Hit::Row(command) => {
                self.recording = false;
                self.conflict = None;
                self.open = (self.open.as_deref() != Some(&command)).then_some(command);
                None
            }
            Hit::Remove(command, n) => {
                let mut shortcuts = row(&command)?.shortcuts;
                (n < shortcuts.len()).then(|| shortcuts.remove(n));
                Some(vec![(command, Some(shortcuts))])
            }
            Hit::Add(_) => {
                self.recording = true;
                self.conflict = None;
                None
            }
            Hit::Reset(command) => Some(vec![(command, None)]),
            Hit::RemoveOthers => {
                let c = self.conflict.take()?;
                let pressed = chord(&c.keys)?;
                let mut out =
                    vec![(c.command.clone(), Some(with(&row(&c.command)?.shortcuts, &c.keys)))];
                for other in &c.others {
                    let kept = row(other)?
                        .shortcuts
                        .into_iter()
                        .filter(|s| chord(s).as_ref() != Some(&pressed))
                        .collect();
                    out.push((other.clone(), Some(kept)));
                }
                Some(out)
            }
            Hit::Leave => {
                let c = self.conflict.take()?;
                Some(vec![(c.command.clone(), Some(with(&row(&c.command)?.shortcuts, &c.keys)))])
            }
            Hit::Cancel => {
                self.conflict = None;
                None
            }
        }
    }

    /// The mouse over the editor: `true` if what's lit changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// Something clickable under `p`.
    pub fn over(&self, p: Point) -> bool {
        self.hits.iter().any(|(r, _)| r.contains(p))
    }

    /// Not drawn this frame (another section shown): nothing of it can be clicked.
    pub fn forget(&mut self) {
        self.hits.clear();
    }

    /// Draws the search (`focused`: its caret) and the commands from `top`; returns where
    /// it ends.
    pub fn paint(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        (ctx, icons): (&Ctx<'_>, &icons::Icons),
        (x0, top, width, focused): (f32, f32, f32, bool),
        rows: &Arc<Vec<BindingRow>>,
    ) -> f32 {
        self.rows = Arc::clone(rows);
        self.hits.clear();
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        // The search: by name or by keys (the app's search field, × and all).
        let field = Rect::new(x0, top, width, px(36.0));
        let placeholder = i18n::t("shortcuts.filter");
        self.filter.paint(layer, text, icons, ctx, (field, focused), &placeholder);
        let filter = ui_kit::ListFilter::new(self.filter.text());
        let matches = |r: &BindingRow| {
            let texts = [r.label.as_str(), &r.group].into_iter();
            filter.matches_any(
                texts
                    .chain(r.shown.iter().map(String::as_str))
                    .chain(r.shortcuts.iter().map(String::as_str)),
            )
        };
        let font = ctx.ui_font(-0.5);
        let small = ctx.ui_font(-2.0);
        let row_h = px(36.0);
        let mut y = field.max_y() + px(14.0);
        let rows = Arc::clone(rows);
        for (i, row) in rows.iter().filter(|r| matches(r)).enumerate() {
            let r = Rect::new(x0, y, width, row_h);
            let open = self.open.as_deref() == Some(&row.command);
            let lit = self.hover == Some(Hit::Row(row.command.clone()));
            let fill = if open {
                p.accent.with_alpha(0.08)
            } else if lit {
                p.text.with_alpha(0.05)
            } else if i % 2 == 0 {
                p.text.with_alpha(0.025)
            } else {
                kernel::Rgba::TRANSPARENT
            };
            layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(px(6.0)));
            let label = if row.changed { format!("{} •", row.label) } else { row.label.clone() };
            let shaped = text.shape_line(&label, &font);
            let base = ds::optical_baseline(r.min_y(), row_h, &font);
            draw_text(
                layer,
                &shaped,
                r.min_x() + px(12.0),
                base,
                ctx.fade(if row.changed { p.accent } else { p.text }),
            );
            let group = text.shape_line(&row.group, &small);
            draw_text(
                layer,
                &group,
                r.min_x() + px(24.0) + shaped.width,
                ds::optical_baseline(r.min_y(), row_h, &small),
                ctx.fade(p.text_muted),
            );
            let mut right = r.max_x() - px(10.0);
            for keys in row.shown.iter().rev() {
                let cap = ds::keycap(layer, text, ctx, keys, right, r.min_y() + row_h / 2.0, false);
                right = cap.min_x() - px(6.0);
            }
            self.hits.push((r, Hit::Row(row.command.clone())));
            y += row_h;
            if open {
                let area = (x0 + px(12.0), y + px(8.0), width - px(24.0));
                y = self.paint_open(layer, text, (ctx, icons), area, row) + px(10.0);
            }
        }
        y
    }

    /// A pill (a button of the open command) at `x`, centered on `mid`; returns the `x` after.
    fn pill(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        (x, mid): (f32, f32),
        label: &str,
        (hit, selected): (Hit, bool),
    ) -> f32 {
        let hovered = self.hover == Some(hit.clone());
        let r = ds::choice(layer, text, ctx, ds::ChoiceSpec { x, mid, label, selected, hovered });
        self.hits.push((r, hit));
        r.max_x() + ctx.px(8.0)
    }

    /// The open command: its keys (×), add, back to the preset; recording; a conflict.
    fn paint_open(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        (ctx, icons): (&Ctx<'_>, &icons::Icons),
        (x0, top, width): (f32, f32, f32),
        row: &BindingRow,
    ) -> f32 {
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let mid = top + px(17.0);
        let mut x = x0;
        for (n, keys) in row.shown.iter().enumerate() {
            let cap = ds::Keycap::new(keys).paint(layer, text, ctx, x, mid);
            let remove = Hit::Remove(row.command.clone(), n);
            let lit = self.hover == Some(remove.clone());
            let button = ds::clear_button(layer, icons, ctx, cap.max_x() + px(14.0), mid, lit);
            self.hits.push((button, remove));
            x = button.max_x() + px(14.0);
        }
        let (label, recording) = if self.recording {
            (i18n::t("shortcuts.recording"), true)
        } else {
            (i18n::t("shortcuts.add"), false)
        };
        x = self.pill(
            layer,
            text,
            ctx,
            (x, mid),
            &label,
            (Hit::Add(row.command.clone()), recording),
        );
        if row.changed {
            self.pill(
                layer,
                text,
                ctx,
                (x, mid),
                &i18n::t("shortcuts.reset"),
                (Hit::Reset(row.command.clone()), false),
            );
        }
        let mut y = mid + px(24.0);
        let Some(c) = self.conflict.clone().filter(|c| c.command == row.command) else { return y };
        let names: Vec<String> = c
            .others
            .iter()
            .filter_map(|o| self.rows.iter().find(|r| &r.command == o))
            .map(|r| r.label.clone())
            .collect();
        let message = i18n::tr!("shortcuts.conflict", keys = c.shown, names = names.join(", "));
        let font = ctx.ui_font(-1.0);
        for line in ds::wrap(text, &message, &font, width, 4) {
            let shaped = text.shape_line(&line, &font);
            draw_text(layer, &shaped, x0, y + px(14.0), ctx.fade(p.warning));
            y += px(20.0);
        }
        let mid = y + px(20.0);
        let mut x = x0;
        for (label, hit) in [
            ("shortcuts.remove_others", Hit::RemoveOthers),
            ("shortcuts.leave", Hit::Leave),
            ("shortcuts.cancel", Hit::Cancel),
        ] {
            x = self.pill(layer, text, ctx, (x, mid), &i18n::t(label), (hit, false));
        }
        mid + px(24.0)
    }
}

/// `shortcuts` with `spec` added (at the end).
fn with(shortcuts: &[String], spec: &str) -> Vec<String> {
    let mut out = shortcuts.to_vec();
    out.push(spec.to_owned());
    out
}

#[cfg(test)]
#[path = "../tests/unit/shortcuts_editor.rs"]
mod tests;
