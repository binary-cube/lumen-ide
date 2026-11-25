//! Editing: typing, folding, comments, moving code, a view's own undo and keys; the keys
//! as a panel or a view gets them.

use super::*;

impl TabViewer {
    /// Beside the text (Split), clicked last: it takes Select All and Copy for its own
    /// selected text (a Markdown preview), the editor every other key.
    pub(super) fn selects(&self) -> bool {
        self.mode == ViewMode::Split
            && self.kind == ui_workbench::ViewerKind::Preview
            && self.keys
            && self.viewer.selects_in_split()
    }
}

impl Workbench {
    /// The active tab's view taking Select All and Copy beside the text ([`TabViewer::selects`]).
    pub(super) fn selecting_viewer(&mut self) -> Option<&mut TabViewer> {
        if self.focus != Focus::Editor {
            return None;
        }
        self.tabs.get_mut(self.active)?.viewer.as_mut().filter(|v| v.selects())
    }

    /// The active tab's view, when it has the keys: shown without the editor, or beside
    /// it (Split) after a click in it while the editor area has the focus.
    pub(super) fn keyed_viewer(&mut self) -> Option<&mut TabViewer> {
        let editor_focus = self.focus == Focus::Editor;
        let v = self.tabs.get_mut(self.active)?.viewer.as_mut()?;
        (!v.editor_visible() || (editor_focus && v.has_keys())).then_some(v)
    }

    /// A view beside the text (Split) has the keys: the text's own edits (⌘D, a line
    /// deleted) don't run.
    pub fn view_keys_beside_text(&self) -> bool {
        self.focus == Focus::Editor
            && self
                .tabs
                .get(self.active)
                .and_then(|t| t.viewer.as_ref())
                .is_some_and(|v| v.editor_visible() && v.has_keys())
    }

    /// ⌘D for a view standing in for its file (a diagram's shapes duplicated); `None`: not
    /// one, the editor's line is duplicated.
    pub fn viewer_duplicate(&mut self) -> Option<bool> {
        let v = self.keyed_viewer().filter(|v| !v.editor_visible() && !v.viewer.wants_text())?;
        Some(v.viewer.command("duplicate"))
    }

    /// ⌘Z / ⇧⌘Z for the view having the keys: in its text field (a grid's `WHERE`), or in
    /// the view itself when it stands in for its file (a diagram's history); beside the text
    /// without a field, nothing (the text isn't what has the keys).
    /// `None`: the view doesn't have them, the editor's undo goes on.
    pub fn viewer_undo(&mut self, redo: bool) -> Option<bool> {
        // The app's own field having the keys (a dialog's, the find bar's): its own history,
        // never the document behind it.
        if let Some(input) = self.active_input_mut() {
            return Some(if redo { input.redo() } else { input.undo() });
        }
        // A window over everything editing its own field (a module's view).
        if let Some(used) = self.overlay_edit(if redo { "redo" } else { "undo" }) {
            return Some(used);
        }
        if let Some(v) = self.panel_field() {
            return Some(v.command(if redo { "redo" } else { "undo" }));
        }
        let v = self.keyed_viewer()?;
        if v.viewer.wants_text() || !v.editor_visible() {
            return Some(v.viewer.command(if redo { "redo" } else { "undo" }));
        }
        Some(false)
    }

    /// The keyboard is in a file's editor (not the tree, the terminal or a view).
    pub fn editor_focused(&self) -> bool {
        self.focus == Focus::Editor
            && self.tabs.get(self.active).is_some_and(|t| {
                t.doc.path().is_some_and(|p| !is_virtual(p))
                    && t.viewer.as_ref().is_none_or(|v| !v.has_keys())
            })
    }

    pub fn fold(&mut self, command: FoldCommand) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let line = tab.doc.buffer().line_of(tab.doc.selections().primary().head) as u32;
        let changed = match command {
            FoldCommand::Fold => tab.folds.collapse_around(line),
            FoldCommand::Unfold => tab.folds.expand_around(line),
            FoldCommand::FoldAll => {
                tab.folds.collapse_all();
                true
            }
            FoldCommand::UnfoldAll => {
                tab.folds.expand_all();
                true
            }
        };
        tab.cursor_out_of_folds();
        changed
    }

    /// Comments / uncomments according to the language at the cursor (HTML inside PHP gets
    /// `<!-- -->`, JS inside `<script>` gets `//`).
    pub fn toggle_comment(&mut self, text: &mut TextSystem) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let head = tab.doc.selections().primary().head;
        let language = tab
            .syntax
            .snapshot()
            .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(head).clone());
        let descriptor = self
            .support
            .languages
            .get(&language)
            .or_else(|| self.support.languages.get(tab.syntax.language()));
        let (line, block) = descriptor.map_or((None, None), |d| (d.line_comment, d.block_comment));
        match (line, block) {
            (Some(token), _) => self.edit(text, |doc| commands::toggle_line_comment(doc, token)),
            (None, Some((open, close))) => {
                self.edit(text, |doc| commands::toggle_block_comment(doc, open, close))
            }
            (None, None) => false,
        }
    }

    // ─── Files and project ───

    /// Applies an operation to the active document and brings the cursor into the viewport.
    /// A collapsed region is taken whole (`Tab::folded`).
    pub fn edit(&mut self, text: &mut TextSystem, f: impl FnOnce(&mut Document)) -> bool {
        self.edit_folded(text, true, f)
    }

    /// [`Self::edit`]; `widen`: a selection reaching a collapsed region covers it (not for
    /// what only moves the cursor: it would land in the region and open it).
    pub(super) fn edit_folded(
        &mut self,
        text: &mut TextSystem,
        widen: bool,
        f: impl FnOnce(&mut Document),
    ) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        // No visible editor (image, preview only): nothing is modified.
        if !tab.editor_visible() {
            return false;
        }
        let version = tab.doc.version();
        tab.folded(widen, f);
        let edited = tab.doc.version() != version;
        if edited {
            self.last_edit = Some(std::time::Instant::now());
            // The language servers hear of it before anything asks them about it.
            self.announce_documents();
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        tab.sync_folds();
        tab.view.set_preedit(None);
        tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
        true
    }

    /// A jump (Go to Line, a definition, a search result, a link): the cursor moved by `f`,
    /// its line brought to the middle of the editor when it's off screen or near an edge.
    pub fn jump(&mut self, text: &mut TextSystem, f: impl FnOnce(&mut Document)) -> bool {
        if !self.edit_folded(text, false, f) {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        tab.view.center_cursor(&tab.doc, &tab.folds, text, &self.palette);
        true
    }

    /// "Move Statement" (`statements`) / "Move Line": the lines at the cursor past their
    /// neighbours, up or down. A statement moves whole — a function with its doc comment,
    /// an element, a rule, as its language's module says — and stays in its block. Only in
    /// the editor: elsewhere (a search field) the keys select as they always did.
    pub fn move_code(&mut self, text: &mut TextSystem, up: bool, statements: bool) -> bool {
        let in_editor = self.focus == Focus::Editor
            && !self.keyboard_in_overlay()
            && self.tabs.get(self.active).is_some_and(|t| t.editor_visible());
        if !in_editor {
            let motion = match (statements && cfg!(target_os = "macos"), up) {
                (true, true) => Motion::DocumentStart,
                (true, false) => Motion::DocumentEnd,
                (false, true) => Motion::Up,
                (false, false) => Motion::Down,
            };
            return self.edit_key(text, EditKey::Move { motion, extend: true });
        }
        // The document's own language (in a PHP file: PHP, its HTML parts included).
        let intelligence = statements
            .then(|| {
                let language = self.tabs.get(self.active)?.syntax.language().clone();
                self.support.intelligence.iter().find(|i| i.language() == language).cloned()
            })
            .flatten();
        self.edit(text, |doc| {
            use code_intelligence::StatementMove;
            let lines = commands::selected_lines(doc);
            let plan = intelligence.map_or(StatementMove::Lines, |i| {
                i.statement_move(&doc.buffer().text(), lines, up)
            });
            match plan {
                StatementMove::Past { unit, past } => {
                    commands::swap_lines(doc, unit, past);
                }
                StatementMove::Stay => {}
                StatementMove::Lines => {
                    commands::move_lines(doc, up);
                }
            }
        })
    }

    /// Typed text (or from the IME): into the search field if it has focus, else the document.
    pub fn type_text(&mut self, text: &mut TextSystem, typed: &str) -> bool {
        self.code_hover.hide();
        // The overlay on top takes the text (About: nowhere to type).
        if let Some(o) = self.active_overlay() {
            return o.slot().text(self, text, typed);
        }
        if self.home_visible() && self.focus == Focus::Editor {
            self.home.picker.type_text(typed);
            return true;
        }
        if let Some(side) = self.focused_side() {
            if side.panel.wants_text() {
                return side.panel.text_input(typed);
            }
            // Its rows have the keyboard (the tree's files): the panel may take it (the
            // tree searches), else typing is for the editor.
            if side.panel.text_on_rows(typed) {
                return true;
            }
            self.move_focus(Focus::Editor);
        }
        // A bottom view taking text (the terminal, a filter field).
        if let Some(used) = self.type_into_view(typed) {
            return used;
        }
        let sink = self.find_sink();
        if self.focus == Focus::Find
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.find_bar.focused_input().insert(typed);
            if tab.find_bar.focus() == FindField::Query {
                tab.update_query(&sink);
                tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
            }
            return true;
        }
        // A viewer without an editor (image, preview only), or one having the keys beside
        // it (a grid in Split), gets its own keys.
        if let Some(v) = self.keyed_viewer() {
            if v.viewer.wants_text() {
                let used = v.viewer.text_input(typed);
                self.take_viewer_action(text);
                return used;
            }
            let mut chars = typed.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => v.viewer.key(c),
                _ => false,
            };
        }
        // Locked: nothing typed (nor completion offered for it).
        if self.refuse_if_locked() {
            self.completion.close();
            return false;
        }
        // A single character: pairs are closed automatically (brackets, quotes, tags).
        let mut single = typed.chars();
        let changed = match (single.next(), single.next()) {
            (Some(c), None) => {
                // What the language just inserted, taken back by this one (`<?=`).
                self.retract_typed(text, c);
                let quotes = self
                    .language_at_cursor()
                    .and_then(|l| self.support.languages.get(&l))
                    .map_or(&['"', '\''][..], |l| l.quotes);
                let changed = self.edit(text, |doc| commands::type_char(doc, c, quotes));
                self.auto_close(text, c);
                self.outdent_closing_tag(text, c);
                changed
            }
            _ => self.edit(text, |doc| commands::insert_text(doc, typed)),
        };
        // Completion is recomputed on characters that can start a suggestion (a name's, and
        // those the language at the cursor says: `$`, `->`, `.`, `<`, quotes…); the language
        // decides whether there's anything to suggest. Any other character closes it.
        let triggers = self.intelligence().map_or("", |i| i.completion_triggers());
        let head = self.tabs.get(self.active).map_or(0, |t| t.doc.selections().primary().head);
        let served = self.served_triggers(head) + &self.assist_triggers(head);
        let mut chars = typed.chars();
        let trigger = matches!((chars.next(), chars.next()), (Some(c), None)
            if c.is_alphanumeric() || c == '_' || triggers.contains(c) || served.contains(c));
        if trigger {
            self.update_completion(text);
        } else {
            self.completion.close();
            self.dismiss_served_completion();
        }
        self.update_param_hint(matches!(typed, "(" | ","));
        changed
    }
}

/// An editing key as a panel gets it: with its reach (by word, extending the selection).
pub(super) fn panel_key(
    key: &EditKey,
) -> Option<(ui_workbench::ViewerKey, ui_workbench::KeyReach)> {
    use ui_workbench::{KeyReach, ViewerKey};
    Some(match *key {
        EditKey::Backspace { word } => (ViewerKey::Backspace, KeyReach { word, extend: false }),
        EditKey::Move { motion: Motion::WordLeft, extend } => {
            (ViewerKey::Left, KeyReach { word: true, extend })
        }
        EditKey::Move { motion: Motion::WordRight, extend } => {
            (ViewerKey::Right, KeyReach { word: true, extend })
        }
        EditKey::Move { motion: Motion::DocumentStart, extend } => {
            (ViewerKey::Home, KeyReach { word: false, extend })
        }
        EditKey::Move { motion: Motion::DocumentEnd, extend } => {
            (ViewerKey::End, KeyReach { word: false, extend })
        }
        EditKey::Move { extend, .. } => (viewer_key(key)?, KeyReach { word: false, extend }),
        // ⇧Tab goes back: the views read it as the reach extended, as the tabs' views do.
        EditKey::Tab { shift } => (ViewerKey::Tab, KeyReach { word: false, extend: shift }),
        _ => (viewer_key(key)?, KeyReach::default()),
    })
}

/// A text field's cursor keys (←→, a word, Home / End), the same in every field. `false`
/// if `motion` isn't one of them.
pub(super) fn move_cursor(input: &mut ui_kit::TextInput, motion: Motion, extend: bool) -> bool {
    match motion {
        Motion::Left => input.move_left(false, extend),
        Motion::Right => input.move_right(false, extend),
        Motion::WordLeft => input.move_left(true, extend),
        Motion::WordRight => input.move_right(true, extend),
        Motion::LineStart | Motion::DocumentStart => input.home(extend),
        Motion::LineEnd | Motion::DocumentEnd => input.end(extend),
        _ => return false,
    }
    true
}

/// An editing key as a view gets it (`None`: not one a view takes).
pub(super) fn viewer_key(key: &EditKey) -> Option<ui_workbench::ViewerKey> {
    use ui_workbench::ViewerKey;
    Some(match key {
        EditKey::Backspace { .. } => ViewerKey::Backspace,
        EditKey::Delete => ViewerKey::Delete,
        EditKey::Enter { .. } => ViewerKey::Enter,
        EditKey::Tab { .. } => ViewerKey::Tab,
        EditKey::Escape => ViewerKey::Escape,
        EditKey::Move { motion, .. } => match motion {
            Motion::Left => ViewerKey::Left,
            Motion::Right => ViewerKey::Right,
            Motion::Up => ViewerKey::Up,
            Motion::Down => ViewerKey::Down,
            Motion::LineStart => ViewerKey::Home,
            Motion::LineEnd => ViewerKey::End,
            Motion::PageUp(_) => ViewerKey::PageUp,
            Motion::PageDown(_) => ViewerKey::PageDown,
            _ => return None,
        },
    })
}
