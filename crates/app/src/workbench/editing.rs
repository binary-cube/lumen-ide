//! Editing: typing, editing keys, clipboard, emmet, comments, find.

use super::*;

impl TabViewer {
    /// Beside the text (Split), clicked last: it takes Select All and Copy for its own
    /// selected text (a Markdown preview), the editor every other key.
    fn selects(&self) -> bool {
        self.mode == ViewMode::Split
            && self.kind == ui_workbench::ViewerKind::Preview
            && self.keys
            && self.viewer.selects_in_split()
    }
}

impl Workbench {
    /// The active tab's view taking Select All and Copy beside the text ([`TabViewer::selects`]).
    fn selecting_viewer(&mut self) -> Option<&mut TabViewer> {
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
    fn edit_folded(
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
            self.focus = Focus::Editor;
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

    pub fn edit_key(&mut self, text: &mut TextSystem, key: EditKey) -> bool {
        self.code_hover.hide();
        if let Some(used) = self.key_overlays(text, key) {
            return used;
        }
        self.key_editor(text, key)
    }

    /// A key while an overlay or a text field has the keyboard (pickers, dialogs, menus,
    /// settings, searches, the tree's new entry). `None`: the editor gets it.
    fn key_overlays(&mut self, text: &mut TextSystem, key: EditKey) -> Option<bool> {
        // Esc with the mouse over the notifications closes them (they never have the
        // keyboard otherwise).
        if key == EditKey::Escape && self.escape_toasts() {
            return Some(true);
        }
        // The overlays, topmost first: the first that keeps the key gets it (a modal keeps
        // them all); what they all let go on goes down the focus chain below.
        if let Some(o) = self.overlay_for_key(&key) {
            let slot = o.slot();
            // Common to all: Esc first clears a search typed in it; the cursor keys it
            // doesn't use move in its field.
            if key == EditKey::Escape && slot.clear_query(self) {
                return Some(true);
            }
            if slot.key(self, text, key) {
                return Some(true);
            }
            return Some(slot.has_input(self) && self.move_in_field(&key));
        }
        // The side panel has the keyboard (the Task Manager's filter, its rows).
        if let Some((viewer_key, reach)) = panel_key(&key)
            && let Some(side) = self.focused_side()
        {
            let used = side.panel.edit_key_with(viewer_key, reach);
            let action = side.panel.take_action();
            let keeps = side.panel.keeps_keyboard();
            if let Some(action) = action {
                let focus = self.focus;
                self.side_action(text, action);
                if keeps {
                    self.focus = focus;
                }
            }
            return Some(used);
        }
        // The bottom tool window's view has the keyboard (↑ ↓ ↵ Esc through the Git log).
        if self.focus == Focus::Panel
            && let Some((viewer_key, reach)) = panel_key(&key)
            && let Some(used) = self.tool_window_key(text, viewer_key, reach)
        {
            return Some(used);
        }
        if self.focus == Focus::Editor
            && let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && v.has_keys()
        {
            // A view without text (the Git log, the local history) gets the navigation keys
            // too; what it doesn't use goes on as before.
            use ui_workbench::ViewerKey;
            let wants_text = v.viewer.wants_text();
            // How far it reaches (⇧ extends, ⌥ / Ctrl by word, ⌘ to the end): a view that
            // reads it (a grid) does; the others get the key as before.
            let plain = ui_workbench::KeyReach::default();
            let reach = |word, extend| ui_workbench::KeyReach { word, extend };
            let (key, reach) = match key {
                EditKey::Backspace { word } => (Some(ViewerKey::Backspace), reach(word, false)),
                EditKey::Delete => (Some(ViewerKey::Delete), plain),
                EditKey::Enter { shift } => (Some(ViewerKey::Enter), reach(false, shift)),
                EditKey::Tab { shift } => (Some(ViewerKey::Tab), reach(false, shift)),
                EditKey::Escape => (Some(ViewerKey::Escape), plain),
                EditKey::Move { motion, extend } => match motion {
                    Motion::Left => (Some(ViewerKey::Left), reach(false, extend)),
                    Motion::Right => (Some(ViewerKey::Right), reach(false, extend)),
                    // By word (⌥ / Ctrl): a field's word moves.
                    Motion::WordLeft => (Some(ViewerKey::Left), reach(true, extend)),
                    Motion::WordRight => (Some(ViewerKey::Right), reach(true, extend)),
                    Motion::Up => (Some(ViewerKey::Up), reach(false, extend)),
                    Motion::Down => (Some(ViewerKey::Down), reach(false, extend)),
                    Motion::LineStart => (Some(ViewerKey::Home), reach(false, extend)),
                    Motion::LineEnd => (Some(ViewerKey::End), reach(false, extend)),
                    Motion::DocumentStart => (Some(ViewerKey::Home), reach(true, extend)),
                    Motion::DocumentEnd => (Some(ViewerKey::End), reach(true, extend)),
                    Motion::PageUp(_) => (Some(ViewerKey::PageUp), reach(false, extend)),
                    Motion::PageDown(_) => (Some(ViewerKey::PageDown), reach(false, extend)),
                },
            };
            let used = key.is_some_and(|k| v.viewer.edit_key_with(k, reach));
            // Beside the text (Split), what the view doesn't use isn't the text's either.
            let beside = v.editor_visible();
            self.take_viewer_action(text);
            if used || wants_text || beside {
                return Some(used);
            }
        }
        // The empty screen (nothing open): the project list gets the keys.
        if self.home_visible() && self.focus == Focus::Editor {
            match key {
                EditKey::Escape if self.home.picker.has_query() => self.home.picker.clear_query(),
                EditKey::Move { motion: Motion::Up, .. } | EditKey::Tab { shift: true } => {
                    self.home.picker.step(-1)
                }
                EditKey::Move { motion: Motion::Down, .. } | EditKey::Tab { shift: false } => {
                    self.home.picker.step(1)
                }
                EditKey::Backspace { .. } => self.home.picker.backspace(),
                // ⌦ on a recent project: off the list (its menu's "Remove from list"; the
                // project stays where it is).
                EditKey::Delete => {
                    let Some(ui_workbench::ProjectsAction::OpenRecent(path)) =
                        self.home.picker.confirm()
                    else {
                        return Some(false);
                    };
                    self.change_recent(|list| list.forget(&path));
                }
                EditKey::Move { motion: Motion::Left, .. }
                    if self.fold_selected_group(false, true) => {}
                EditKey::Move { motion: Motion::Right, .. }
                    if self.fold_selected_group(false, false) => {}
                EditKey::Enter { .. } => {
                    let action = self.home.picker.confirm();
                    self.request = action.map(|a| self.picker_action(a, false));
                }
                _ => return Some(false),
            }
            return Some(true);
        }
        if self.focus == Focus::Find {
            return Some(self.find_key(text, key));
        }
        None
    }

    /// A key in the editor: completion, movement, doc comments, expansions, edits.
    fn key_editor(&mut self, text: &mut TextSystem, key: EditKey) -> bool {
        let mut refresh_completion = false;
        if self.completion.is_open() {
            match key {
                EditKey::Move { motion: Motion::Up, extend: false } => {
                    self.completion.move_selection(-1);
                    return true;
                }
                EditKey::Move { motion: Motion::Down, extend: false } => {
                    self.completion.move_selection(1);
                    return true;
                }
                EditKey::Move { motion: Motion::PageUp(_), .. } => {
                    self.completion.move_selection(-8);
                    return true;
                }
                EditKey::Move { motion: Motion::PageDown(_), .. } => {
                    self.completion.move_selection(8);
                    return true;
                }
                EditKey::Enter { shift: false } | EditKey::Tab { shift: false } => {
                    return self.accept_completion(text);
                }
                EditKey::Escape => {
                    self.completion.close();
                    self.dismiss_served_completion();
                    return true;
                }
                EditKey::Backspace { .. } => refresh_completion = true,
                _ => {
                    self.completion.close();
                    self.dismiss_served_completion();
                }
            }
        }
        // A list still waited for from a server (not shown yet): it never shows.
        if key == EditKey::Escape {
            self.dismiss_served_completion();
        }
        if key == EditKey::Escape && self.param_hint.hide() {
            return true;
        }
        let page = self.tabs.get(self.active).map_or(10, |t| t.view.page_lines());
        if let EditKey::Move { motion, extend } = key
            && let Some(tab) = self.tabs.get_mut(self.active)
            && tab.folds.has_any()
        {
            // With collapsed regions, ↑ / ↓ / PageUp / PageDown move over displayed lines.
            let (down, steps) = match motion {
                Motion::Up => (false, 1),
                Motion::Down => (true, 1),
                Motion::PageUp(_) => (false, page),
                Motion::PageDown(_) => (true, page),
                _ => (false, 0),
            };
            if steps > 0 {
                let folds = &tab.folds;
                let lines = tab.doc.buffer().line_count() as u32;
                commands::move_vertical(&mut tab.doc, down, extend, &|line| {
                    let mut current = line as u32;
                    for _ in 0..steps {
                        match folds.next_visible(current, down, lines) {
                            Some(next) => current = next,
                            None if current == line as u32 => return None,
                            None => break,
                        }
                    }
                    Some(current as usize)
                });
                tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
                return true;
            }
        }
        // The language's doc block opener (`/**`) + Enter above a declaration: the block is
        // filled in from the signature.
        let opener = matches!(key, EditKey::Enter { .. })
            .then(|| self.intelligence().and_then(|i| i.doc_rules()).map(|r| r.opener))
            .flatten();
        let doc_stub = opener.is_some_and(|opener| {
            self.tabs.get(self.active).is_some_and(|tab| {
                let doc = &tab.doc;
                let head = doc.selections().primary().head;
                let buffer = doc.buffer();
                let line = buffer.line_of(head);
                doc.selections().len() == 1
                    && buffer.slice(buffer.line_start(line)..head).trim_end().ends_with(opener)
                    && buffer.slice(head..buffer.line_end(line)).trim().is_empty()
            })
        });
        // Tab: expansions first (e.g. Emmet in HTML), then the usual indentation.
        if matches!(key, EditKey::Tab { shift: false }) && self.expand_abbreviation(text) {
            return true;
        }
        // Enter: indented by the syntax tree when it knows.
        let bases =
            if matches!(key, EditKey::Enter { .. }) { self.enter_bases() } else { Vec::new() };
        let base_of =
            |offset: usize| bases.iter().find(|(o, _)| *o == offset).and_then(|(_, b)| b.clone());
        let moves = matches!(key, EditKey::Move { .. } | EditKey::Escape);
        let changed = self.edit_folded(text, !moves, |doc| match key {
            EditKey::Move { motion, extend } => {
                let motion = match motion {
                    Motion::PageUp(_) => Motion::PageUp(page),
                    Motion::PageDown(_) => Motion::PageDown(page),
                    m => m,
                };
                commands::move_cursors(doc, motion, extend);
            }
            EditKey::Enter { .. } => commands::newline_with(doc, &base_of),
            EditKey::Backspace { word: true } => commands::delete_word_left(doc),
            EditKey::Backspace { word: false } => commands::backspace(doc),
            EditKey::Delete => commands::delete_forward(doc),
            EditKey::Tab { shift: false } => commands::indent(doc),
            EditKey::Tab { shift: true } => commands::outdent(doc),
            EditKey::Escape => {
                commands::collapse_to_primary(doc);
            }
        });
        if doc_stub {
            self.fill_doc_comment(text);
        }
        if refresh_completion {
            // The word erased (nothing typed, no `.` before): the list closes, as it opened.
            // Only where a server answers: the built-in engine decides alone, as before.
            let erased = self.tabs.get(self.active).is_some_and(|t| {
                let head = t.doc.selections().primary().head;
                self.served_at(head).uses_server() && !self.suggests_after_erase(&t.text(), head)
            });
            if erased {
                self.completion.close();
                self.dismiss_served_completion();
            } else {
                self.update_completion(text);
            }
        }
        if changed {
            self.update_param_hint(false);
        }
        changed
    }

    /// Expands the abbreviation before the cursor (Emmet etc.), if the language at the cursor
    /// has an expander and there really is an abbreviation there. `true` if it expanded.
    pub(super) fn expand_abbreviation(&mut self, text: &mut TextSystem) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let doc = &tab.doc;
        let selection = doc.selections().primary();
        if doc.selections().len() != 1 || !selection.is_empty() {
            return false;
        }
        let head = selection.head;
        let buffer = doc.buffer();
        let line_start = buffer.line_start(buffer.line_of(head));
        let line_before = buffer.slice(line_start..head);
        // The language at the start of the line's text: the abbreviation just typed may not be in
        // the last parse yet (e.g. HTML inside PHP, where PHP would show up at the cursor).
        let at = line_start + (line_before.len() - line_before.trim_start().len());
        let language = tab
            .syntax
            .snapshot()
            .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(at.min(head)).clone());
        let Some(expander) =
            self.support.expanders.iter().find(|e| e.languages().contains(&language.as_str()))
        else {
            return false;
        };
        let indent: String = line_before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let unit = doc.indent().unit();
        let Some(expansion) = expander.expand(&line_before, &indent, &unit) else { return false };
        let start = head - expansion.replace;
        self.edit(text, |doc| {
            commands::click(doc, start, false, false);
            commands::click(doc, head, true, false);
            commands::insert_text(doc, &expansion.text);
            commands::click(doc, start + expansion.cursor, false, false);
        })
    }

    /// Fills in the doc block just opened (the cursor on the ` * ` line) with the lines
    /// generated by the language (`@param`, `@return`…), below an empty line for the description.
    pub(super) fn fill_doc_comment(&mut self, text: &mut TextSystem) {
        let Some(intel) = self.intelligence() else { return };
        let Some(tab) = self.tabs.get(self.active) else { return };
        let head = tab.doc.selections().primary().head;
        let lines = intel.doc_comment(&tab.text(), head);
        if lines.is_empty() {
            return;
        }
        let buffer = tab.doc.buffer();
        let line = buffer.line_of(head);
        // The current line's prefix, e.g. `     * `.
        let prefix = buffer.slice(buffer.line_start(line)..head);
        let star = prefix.trim_end();
        let mut insert = format!("\n{star}");
        for l in &lines {
            insert.push_str(&format!("\n{prefix}{l}"));
        }
        self.edit(text, |doc| {
            commands::insert_text(doc, &insert);
            commands::click(doc, head, false, false);
        });
    }

    pub(super) fn find_key(&mut self, text: &mut TextSystem, key: EditKey) -> bool {
        let sink = self.find_sink();
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let bar = &mut tab.find_bar;
        let query_changed = bar.focus() == FindField::Query;
        match key {
            EditKey::Escape => {
                tab.find_open = false;
                self.focus = Focus::Editor;
                return true;
            }
            EditKey::Enter { shift } => {
                if bar.focus() == FindField::Replace {
                    tab.find.replacement = bar.replace.text().to_owned();
                    tab.find.replace_current(&mut tab.doc);
                } else if shift {
                    tab.find.find_prev(&mut tab.doc);
                } else {
                    tab.find.find_next(&mut tab.doc);
                }
                tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
                return true;
            }
            EditKey::Tab { .. } => {
                if bar.show_replace {
                    bar.set_focus(match bar.focus() {
                        FindField::Query => FindField::Replace,
                        FindField::Replace => FindField::Query,
                    });
                }
                return true;
            }
            EditKey::Backspace { word } => bar.focused_input().backspace(word),
            EditKey::Delete => bar.focused_input().delete(),
            EditKey::Move { motion, extend } => {
                move_cursor(bar.focused_input(), motion, extend);
                return true;
            }
        }
        if query_changed {
            tab.update_query(&sink);
            tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
        }
        true
    }

    /// A modal or a window over the editor with a text field has the keyboard: what's
    /// behind it shows no caret (and gets no keys).
    pub(super) fn keyboard_in_overlay(&self) -> bool {
        self.modal_open()
            || self.active_overlay().is_some_and(|o| {
                let slot = o.slot();
                slot.has_input(self) || slot.input(self).is_some()
            })
    }

    /// Who has the keyboard's text field that the app edits: the window over the editor on
    /// top when it has one (none when it hasn't, or its view edits its own: nothing behind
    /// it is edited), else the app's screens (the empty screen's search, the find bar).
    fn input_owner(&self) -> Option<InputOwner> {
        // Down the open windows over the editor: the first with a field has it; a popover
        // without one (a context menu) lets it through to what's under it, a modal never.
        if self.active_overlay().is_some() {
            for o in Overlay::ALL {
                let slot = o.slot();
                if slot.floats(self) || !slot.is_open(self) {
                    continue;
                }
                if slot.input(self).is_some() {
                    return Some(InputOwner::Overlay(o));
                }
                if slot.kind(self) == OverlayKind::Modal {
                    return None;
                }
            }
            return None;
        }
        if self.home_visible() && self.focus == Focus::Editor {
            return Some(InputOwner::Home);
        }
        let find =
            self.focus == Focus::Find && self.tabs.get(self.active).is_some_and(|t| t.find_open);
        find.then_some(InputOwner::Find)
    }

    /// The side panel having the keyboard, while it takes text (its field: the project
    /// panel's search, a new entry's name).
    fn side_typing(&self) -> Option<&SidePanel> {
        let Focus::Side(dock) = self.focus else { return None };
        if self.keyboard_in_overlay() {
            return None;
        }
        self.tools.side_on(dock).filter(|s| s.panel.wants_text())
    }

    /// The text field that has the keyboard (see `input_owner`).
    pub(super) fn active_input(&self) -> Option<&ui_kit::TextInput> {
        Some(match self.input_owner()? {
            InputOwner::Overlay(o) => o.slot().input(self)?,
            InputOwner::Home => &self.home.picker.query.input,
            InputOwner::Find => self.tabs.get(self.active)?.find_bar.focused_input_ref(),
        })
    }

    /// A cursor key for the text field having the keyboard. `true` if it moved in it.
    pub(super) fn move_in_field(&mut self, key: &EditKey) -> bool {
        let EditKey::Move { motion, extend } = *key else { return false };
        self.active_input_mut().is_some_and(|input| move_cursor(input, motion, extend))
    }

    pub(super) fn active_input_mut(&mut self) -> Option<&mut ui_kit::TextInput> {
        Some(match self.input_owner()? {
            InputOwner::Overlay(o) => o.slot().input_mut(self)?,
            InputOwner::Home => &mut self.home.picker.query.input,
            InputOwner::Find => self.tabs.get_mut(self.active)?.find_bar.focused_input(),
        })
    }

    /// The text to copy: from the focused text field, or from the document.
    /// A collapsed region reached is copied whole (`Tab::folded`).
    pub fn copy(&mut self) -> Option<String> {
        match self.selected_outside_document() {
            Some(selected) => selected,
            None => Some(self.tabs.get_mut(self.active)?.folded(true, |doc| commands::copy(doc))),
        }
    }

    /// What's selected where the keyboard is (a field, the terminal, a view, the document);
    /// nothing selected: `None`, never the line a copy would take.
    pub fn selected_text(&self) -> Option<String> {
        let selected = match self.selected_outside_document() {
            Some(selected) => selected,
            None => {
                let doc = &self.tabs.get(self.active)?.doc;
                let selection = doc.selections().primary();
                (!selection.is_empty()).then(|| commands::copy(doc))
            }
        };
        selected.filter(|s| !s.is_empty())
    }

    /// `Some`: what a field or a view having the keyboard holds selected (maybe nothing).
    /// `None`: it's the document's.
    fn selected_outside_document(&self) -> Option<Option<String>> {
        // A window over everything answering the clipboard itself (a module's view): what
        // its field (or its view) has selected, never the document behind it.
        if self.input_owner().is_none()
            && let Some(selected) = self.active_overlay().and_then(|o| o.slot().selected_text(self))
        {
            return Some(selected);
        }
        if let Some(selected) = self.view_selection() {
            return Some(Some(selected).filter(|s| !s.is_empty()));
        }
        // The bottom view's field with nothing selected: nothing (not the editor's text).
        if self.panel_field_focused() {
            return Some(None);
        }
        // The bottom view has the keyboard (the terminal) and nothing selected in it:
        // nothing copied (not what's selected elsewhere).
        if self.view_takes_keyboard() && !self.overlay_open() {
            return Some(None);
        }
        if let Some(side) = self.side_typing() {
            return Some(side.panel.selected_text());
        }
        if self.input_owner().is_some() {
            let selected = self.active_input().map(|input| input.selected_text().to_owned());
            return Some(selected.filter(|s| !s.is_empty()));
        }
        let tab = self.tabs.get(self.active)?;
        // Beside the text, a preview clicked last: its selected text (nothing if none).
        if self.focus == Focus::Editor
            && let Some(v) = tab.viewer.as_ref().filter(|v| v.selects())
        {
            return Some(v.viewer.selected_text());
        }
        // A view having the keys (a grid): what's selected in it.
        if let Some(v) = &tab.viewer
            && (!v.editor_visible() || (self.focus == Focus::Editor && v.has_keys()))
        {
            let selected = v.viewer.selected_text();
            // Beside the text, nothing selected in it copies nothing (not the text's).
            if selected.is_some() || v.editor_visible() {
                return Some(selected);
            }
        }
        None
    }

    /// An edit command for the window on top's own field or view: `None` when it takes none.
    fn overlay_edit(&mut self, id: &str) -> Option<bool> {
        self.active_overlay()?.slot().edit_command(self, id)
    }

    pub fn cut(&mut self, text: &mut TextSystem) -> Option<String> {
        // A window over everything answering the clipboard itself: its field's selection,
        // never the document's.
        if self.input_owner().is_none()
            && let Some(selected) = self.active_overlay().and_then(|o| o.slot().selected_text(self))
        {
            self.overlay_edit("cut");
            return selected;
        }
        if self.input_owner().is_some() || self.side_typing().is_some() {
            let copied = self.copy();
            // Inserting empty text deletes the selection in the field (and refreshes what
            // depends on it, e.g. a filter).
            self.type_text(text, "");
            return copied;
        }
        // The bottom view's field: its selection cut.
        if let Some(v) = self.panel_field() {
            let selected = v.selected_text();
            v.command("cut");
            return selected;
        }
        // A view having the keys: its field's text cut; cells only copied, but in a view
        // standing in for its file (a diagram's shapes): copied, then cut.
        if let Some(v) = self.keyed_viewer() {
            if !v.viewer.wants_text() {
                let selected = v.viewer.selected_text();
                if !v.editor_visible() && selected.is_some() {
                    v.viewer.command("cut");
                }
                return selected;
            }
            let selected = v.viewer.selected_text();
            v.viewer.command("cut");
            return selected;
        }
        let mut copied = None;
        self.edit(text, |doc| copied = Some(commands::cut(doc)));
        copied
    }

    pub fn paste(&mut self, text: &mut TextSystem, clip: &str) -> bool {
        if self.view_takes_keyboard() {
            return self.paste_into_view(clip);
        }
        // The bottom view's field: its first line, typed in (all of it in a field of several
        // lines: a commit's message).
        if let Some(v) = self.panel_field() {
            let line =
                if v.field_takes_lines() { clip } else { clip.lines().next().unwrap_or_default() };
            return v.paste(line) || v.text_input(line);
        }
        if self.input_owner().is_some() || self.side_typing().is_some() {
            return self.type_text(text, clip.lines().next().unwrap_or_default());
        }
        // A window over everything whose view takes what's pasted.
        if let Some(o) = self.active_overlay()
            && let Some(used) = o.slot().paste(self, text, clip.lines().next().unwrap_or_default())
        {
            return used;
        }
        // A viewer with its own field (the commit message, the canvas search).
        if let Some(v) = self.keyed_viewer() {
            // Beside the text, a view without a field takes nothing pasted.
            if v.viewer.wants_text() || !v.editor_visible() {
                return v.viewer.wants_text() && v.viewer.text_input(clip);
            }
        }
        self.edit(text, |doc| commands::paste(doc, clip))
    }

    /// ⌘A: the whole text of the focused field, a viewer's field, or the document.
    pub fn select_all(&mut self, text: &mut TextSystem) -> bool {
        if let Some(input) = self.active_input_mut() {
            input.select_all();
            return true;
        }
        if let Some(used) = self.overlay_edit("select_all") {
            return used;
        }
        // The bottom view's text field (the Git log's filters).
        if let Some(v) = self.panel_field() {
            return v.command("select_all");
        }
        // A viewer's text field (e.g. the canvas search, the theme name).
        if self.focus == Focus::Editor
            && let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && v.has_keys()
            && v.viewer.wants_text()
        {
            return v.viewer.command("select_all");
        }
        // Beside the text, a preview clicked last: all its text.
        if let Some(v) = self.selecting_viewer() {
            v.viewer.select_all();
            return true;
        }
        // A view selecting what it shows (a grid's rows).
        if self.focus == Focus::Editor
            && let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && v.has_keys()
        {
            // Beside the text, the view's ⌘A stays in it.
            if v.viewer.select_all() || v.editor_visible() {
                return true;
            }
        }
        self.edit(text, commands::select_all)
    }

    pub fn set_preedit(&mut self, preedit: Option<String>) -> bool {
        match self.tabs.get_mut(self.active) {
            // Locked: nothing composed in it either (what's committed is refused).
            Some(tab) if self.focus == Focus::Editor && (preedit.is_none() || !tab.locked()) => {
                tab.view.set_preedit(preedit);
                true
            }
            _ => false,
        }
    }

    // ─── Search ───

    /// Opens the Find bar (or Find + Replace). The selected text becomes the search term.
    pub fn open_find(&mut self, text: &mut TextSystem, replace: bool) -> bool {
        let sink = self.find_sink();
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        // A view without the editor that finds in what it shows (a grid's rows).
        if !replace
            && let Some(v) = &mut tab.viewer
            && (!v.editor_visible() || (self.focus == Focus::Editor && v.has_keys()))
            && v.viewer.command("find")
        {
            self.focus = Focus::Editor;
            return true;
        }
        let selection = tab.doc.selections().primary();
        if !selection.is_empty() {
            let selected = tab.doc.buffer().slice(selection.range());
            if !selected.contains('\n') {
                tab.find_bar.query.set_text(&selected);
            }
        }
        tab.find_bar.query.input.select_all();
        tab.find_bar.show_replace |= replace;
        tab.find_bar.set_focus(if replace && !tab.find_bar.query.text().is_empty() {
            FindField::Replace
        } else {
            FindField::Query
        });
        tab.find_open = true;
        self.focus = Focus::Find;
        tab.update_query(&sink);
        tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
        true
    }

    pub fn find_step(&mut self, text: &mut TextSystem, forward: bool) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        if tab.find.query.pattern.is_empty() {
            return false;
        }
        if forward {
            tab.find.find_next(&mut tab.doc)
        } else {
            tab.find.find_prev(&mut tab.doc)
        }
        tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
        true
    }

    pub(super) fn find_hit(&mut self, text: &mut TextSystem, hit: FindHit) {
        let sink = self.find_sink();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let mut query = tab.find.query.clone();
        match hit {
            FindHit::Field(field) => {
                tab.find_bar.set_focus(field);
                self.focus = Focus::Find;
                return;
            }
            FindHit::ToggleReplace => {
                tab.find_bar.show_replace = !tab.find_bar.show_replace;
                tab.sync_bounds(self.editor_area, self.scale);
                return;
            }
            FindHit::Clear => {
                tab.find_bar.query.set_text("");
                query.pattern.clear();
                tab.find_bar.set_focus(FindField::Query);
                self.focus = Focus::Find;
            }
            FindHit::ToggleCase => query.case_sensitive = !query.case_sensitive,
            FindHit::ToggleWord => query.whole_word = !query.whole_word,
            FindHit::ToggleRegex => query.regex = !query.regex,
            FindHit::Prev => tab.find.find_prev(&mut tab.doc),
            FindHit::Next => tab.find.find_next(&mut tab.doc),
            FindHit::Close => {
                tab.find_open = false;
                self.focus = Focus::Editor;
                return;
            }
            FindHit::ReplaceOne => {
                tab.find.replacement = tab.find_bar.replace.text().to_owned();
                tab.find.replace_current(&mut tab.doc);
            }
            FindHit::ReplaceAll => {
                tab.find.replacement = tab.find_bar.replace.text().to_owned();
                let n = tab.find.replace_all(&mut tab.doc);
                tracing::debug!(count = n, "replacements");
            }
        }
        if query != tab.find.query {
            tab.find.set_query(query);
            sink.start(tab);
            tab.find.select_from_cursor(&mut tab.doc);
        }
        tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
    }

    // ─── Mouse ───
}

/// Who has the keyboard's text field (see `Workbench::input_owner`): a window over the
/// editor (its slot says which field), or one of the app's screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputOwner {
    Overlay(Overlay),
    Home,
    Find,
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
