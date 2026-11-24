//! The find bar: opened with the selection, its keys, the next or previous match, a match
//! shown.

use super::editing::move_cursor;
use super::*;

impl Workbench {
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
}
