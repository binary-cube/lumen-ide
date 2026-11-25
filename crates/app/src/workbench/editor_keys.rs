//! The editing keys: first to what has the keyboard over the editor (pickers, dialogs,
//! menus, searches, the tree's new entry), then to the editor (completion, movement, doc
//! comments, expansions, edits).

use super::editing::panel_key;
use super::*;

impl Workbench {
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
                    self.move_focus(focus);
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
        // Lines wrapping: Home / End go to the row's start and end first (a second press,
        // the line's).
        if let EditKey::Move { motion: motion @ (Motion::LineStart | Motion::LineEnd), extend } =
            key
            && let Some(tab) = self.tabs.get_mut(self.active)
            && tab.view.wraps()
        {
            let end = motion == Motion::LineEnd;
            let (view, doc, folds) = (&mut tab.view, &tab.doc, &tab.folds);
            let targets: Vec<Option<usize>> = doc
                .selections()
                .iter()
                .map(|s| view.row_home_end(doc, folds, s.head, end))
                .collect();
            if targets.iter().any(Option::is_some) {
                let mut targets = targets.into_iter();
                let moved = doc.selections().transform(|s| {
                    let Some(Some(head)) = targets.next() else { return *s };
                    let anchor = if extend { s.anchor } else { head };
                    document::Selection { anchor, head, goal: None }
                });
                tab.doc.set_selections(moved);
                tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
                return true;
            }
        }
        // Lines wrapping: ↑ / ↓ / PageUp / PageDown move over the rows shown, not the lines.
        if let EditKey::Move { motion, extend } = key
            && let Some(tab) = self.tabs.get_mut(self.active)
            && tab.view.wraps()
        {
            let rows = match motion {
                Motion::Up => -1,
                Motion::Down => 1,
                Motion::PageUp(_) => -i64::from(page),
                Motion::PageDown(_) => i64::from(page),
                _ => 0,
            };
            if rows != 0 {
                let (view, doc, folds) = (&mut tab.view, &tab.doc, &tab.folds);
                let palette = &self.palette;
                let moved = doc.selections().transform(|s| {
                    let goal = s.goal.map(|g| g as usize);
                    let step = ui_workbench::RowMove { head: s.head, goal, rows };
                    let (head, goal) = view.moved_by_rows((doc, folds), (text, palette), step);
                    let anchor = if extend { s.anchor } else { head };
                    let goal = u32::try_from(goal).unwrap_or(u32::MAX);
                    document::Selection { anchor, head, goal: Some(goal) }
                });
                tab.doc.set_selections(moved);
                tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
                return true;
            }
        }
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
}
