//! The in-code color picker, ⌘ links, error tooltips and the code hover card.

use super::*;

impl Workbench {
    /// Click on a color swatch in the gutter: opens the colors module's picker (a popover
    /// next to it). `true` if it did.
    pub(super) fn open_color_picker_at(&mut self, p: Point) -> bool {
        let Some(tab) = self.tabs.get(self.active).filter(|t| t.editor_visible()) else {
            return false;
        };
        let Some(line) = tab.view.swatch_at(p) else { return false };
        let Some(anchor) = tab.view.swatch_rect(line) else { return false };
        let Some(swatches) = self.swatches.value() else { return false };
        let Some(i) = swatches.lines.iter().position(|(l, _)| *l == line) else { return false };
        let literal = &swatches.literals[i];
        let pick = ui_workbench::ColorPick {
            anchor,
            color: literal.color,
            format: literal.format,
            range: literal.range.clone(),
        };
        if !self.open_bare_view(ui_workbench::PICKER_EXTENSION, &pick, true) {
            return false;
        }
        self.tooltips.hide();
        true
    }

    #[cfg(feature = "snapshot")]
    /// Opens the picker on the color of line `line` (diagnostics / snapshots).
    pub fn open_color_picker_line(&mut self, line: u32) -> bool {
        let rect = self.tabs.get(self.active).and_then(|t| t.view.swatch_rect(line));
        rect.is_some_and(|r| {
            self.open_color_picker_at(Point::new(
                r.min_x() + r.size.width / 2.0,
                r.min_y() + r.size.height / 2.0,
            ))
        })
    }

    /// Replaces `range` of the active file with `value`, selected after (one undo step), the
    /// editor having the keyboard again (a color picked for its literal). `true` if it did.
    pub fn replace_in_active(
        &mut self,
        text: &mut TextSystem,
        range: std::ops::Range<usize>,
        value: &str,
    ) -> bool {
        if self.tabs.get(self.active).is_none_or(|t| !t.editor_visible()) {
            return false;
        }
        self.focus = Focus::Editor;
        self.edit(text, |doc| {
            let len = doc.buffer().len();
            let (start, end) = (range.start.min(len), range.end.min(len));
            commands::click(doc, start, false, false);
            commands::click(doc, end, true, false);
            commands::insert_text(doc, value);
        });
        true
    }

    /// The color picked with the screen eyedropper (`None`: cancelled): to the view that
    /// asked (the color picker over the window, the theme studio's picker).
    pub fn screen_color(&mut self, color: Option<Rgba>) -> bool {
        let Some(color) = color else { return false };
        self.viewer_command(&format!("screen-color:{}", color.to_hex()))
    }

    // ─── ⌘ + hover: links to declarations ───

    /// ⌘ (Ctrl) was pressed / released; `p` is the mouse position. `true` = needs a redraw.
    pub fn set_link_mode(&mut self, text: &mut TextSystem, on: bool, p: Point) -> bool {
        self.link_mode = on && !self.modal_open();
        // With ⌘ held, the card of what's under the mouse appears at once.
        let card = self.update_code_hover(text, p);
        // A bottom view's links (the terminal's) too.
        let over = self.link_mode && self.tools.bottom_bounds.contains(p);
        let panel = self.panel_link_hover(over.then_some(p));
        self.update_link(text, p) || card || panel
    }

    /// Recomputes the error under the mouse (the tooltip with its reason). `true` = redraw.
    pub fn update_error_tip(&mut self, text: &mut TextSystem, p: Point) -> bool {
        let before = self.error_tip;
        let now = self.error_at(text, p);
        // The tooltip stays where it appeared while the mouse is on the same error: no
        // redraw on every move.
        self.error_tip = match before {
            Some((i, at)) if Some(i) == now => Some((i, at)),
            _ => now.map(|i| (i, p)),
        };
        self.error_tip.map(|(i, _)| i) != before.map(|(i, _)| i)
    }

    /// The mouse moved: the word under it, for the code hover card (an error's tooltip
    /// wins). `true` if a redraw is needed.
    pub fn update_code_hover(&mut self, text: &mut TextSystem, p: Point) -> bool {
        let (word, over) = self.code_hover_word(text, p);
        let revealed = self.set_pointer_text(over);
        self.code_hover.track(word, p, self.link_mode) || revealed
    }

    /// The word under `p` for the hover card, and the byte of the text under it (`None`:
    /// not on the active editor's text).
    fn code_hover_word(
        &mut self,
        text: &mut TextSystem,
        p: Point,
    ) -> (Option<std::ops::Range<usize>>, Option<usize>) {
        if self.overlay_open() {
            return (None, None);
        }
        let Some((offset, start, end)) = self.text_under(text, p) else { return (None, None) };
        if self.error_tip.is_some() || self.completion.is_open() {
            return (None, Some(offset));
        }
        let tab = &self.tabs[self.active];
        let source = tab.doc.buffer().slice(start..end);
        // The word characters of the language there (`-` in CSS, `$` and `\` in PHP).
        let language = tab
            .syntax
            .snapshot()
            .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(offset).clone());
        let intelligence = self.support.intelligence.iter().find(|i| i.language() == language);
        let extra = match intelligence {
            Some(i) => i.word_chars(),
            None => self.support.languages.get(&language).map_or("", |d| d.word_chars),
        };
        let word = code_intelligence::word_at(&source, offset - start, extra);
        (word.map(|w| start + w.start..start + w.end), Some(offset))
    }

    /// The byte of the active editor's text under `p`, with its line's start and end: right
    /// on the text (not past a line's end).
    fn text_under(&mut self, text: &mut TextSystem, p: Point) -> Option<(usize, usize, usize)> {
        let tab = self.tabs.get_mut(self.active).filter(|t| t.editor_visible())?;
        if !tab.view.bounds().contains(p)
            || tab.view.over_scrollbar(p)
            || (tab.find_open && tab.find_bar.contains(p))
        {
            return None;
        }
        let offset = tab.view.offset_at(p, &tab.doc, &tab.folds, text, &self.palette);
        let buffer = tab.doc.buffer();
        let line = buffer.line_of(offset);
        let (start, end) = (buffer.line_start(line), buffer.line_end(line));
        (offset < end).then_some((offset, start, end))
    }

    /// The pending card is due: computes it (once) from the language intelligence.
    pub(super) fn resolve_code_hover(&mut self) {
        let Some((word, _)) = self.code_hover.due() else { return };
        // A language server's card (asked: shown with its answer; nothing to say, an error,
        // a timeout: the built-in engine's) — unless a module or a framework explains the
        // word (a console's table, an htmx attribute, a React hook): theirs, as before.
        if self.served_at(word.start).uses_server() && !self.explained_by_modules(word.start) {
            match self.served_hover(word.start) {
                Some(None) => return,
                Some(Some(Some(info))) => {
                    let card = self.hover_card(info, word.start);
                    self.code_hover.show(card);
                    return;
                }
                Some(Some(None)) | None => {}
            }
        }
        let card = self.code_hover_card(word.start);
        self.code_hover.show(card);
    }

    /// Whether a module's assist or a framework has a card for the word at `offset` (no
    /// language server knows what they add).
    fn explained_by_modules(&self, offset: usize) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let source = tab.text();
        self.assist_hover(&source, offset).is_some()
            || self.framework_hover(&source, offset).is_some()
    }

    pub(super) fn code_hover_card(&self, offset: usize) -> Option<ui_workbench::HoverCard> {
        let tab = self.tabs.get(self.active)?;
        let source = tab.text();
        let path = Self::index_path(tab);
        let info = match self
            .assist_hover(&source, offset)
            .or_else(|| self.framework_hover(&source, offset))
        {
            Some(info) => info,
            // Another language's name an assist found here (a PHP class in a template).
            None if let Some((intel, target)) = self.assist_code(&source, offset) => {
                let index = self.data.symbols.try_read().ok()?;
                self.with_project(|project| {
                    code_intelligence::hover_of(
                        intel.as_ref(),
                        &index,
                        &path,
                        &source,
                        offset,
                        &target,
                        project,
                    )
                })?
            }
            None => {
                let intel = self.intelligence_at(offset)?;
                // The index may be busy (being written): no card rather than waiting.
                let index = self.data.symbols.try_read().ok()?;
                self.with_project(|project| {
                    code_intelligence::hover(
                        intel.as_ref(),
                        &index,
                        &path,
                        &source,
                        offset,
                        project,
                    )
                })?
            }
        };
        self.hover_card(info, offset)
    }

    /// What `info` (the word at `offset`) shows on the card.
    fn hover_card(
        &self,
        info: code_intelligence::HoverInfo,
        offset: usize,
    ) -> Option<ui_workbench::HoverCard> {
        let intel = self.intelligence_at(offset);
        let tab = self.tabs.get(self.active)?;
        let language = intel
            .as_ref()
            .map_or_else(|| tab.syntax.language().to_string(), |i| i.language().to_string());
        let language = language.to_uppercase();
        let notes = info
            .notes
            .iter()
            .map(|note| {
                use code_intelligence::HoverNote as N;
                let language = language.as_str();
                match note {
                    N::Since(v) => (tr!("hover.since", language = language, version = *v), false),
                    N::Removed(v) => {
                        (tr!("hover.removed", language = language, version = *v), true)
                    }
                    N::Deprecated => (i18n::t("hover.deprecated"), true),
                    N::Experimental => (i18n::t("hover.experimental"), false),
                    N::NonStandard => (i18n::t("hover.non_standard"), true),
                    N::Extension(name) => (tr!("hover.extension", name = *name), false),
                }
            })
            .collect();
        Some(ui_workbench::HoverCard {
            title: info.title,
            subtitle: info.subtitle,
            code: code_intelligence::multiline_signature(&info.code),
            doc: info.doc,
            notes,
            params: info
                .params
                .into_iter()
                .map(|p| ui_workbench::CardParam { ty: p.ty, name: p.name, doc: p.doc })
                .collect(),
            source: info.source,
            extension: intel
                .and_then(|i| i.extensions().first().map(|e| (*e).to_owned()))
                .unwrap_or_default(),
            labels: (
                info.params_label.unwrap_or_else(|| i18n::t("hover.parameters")),
                i18n::t("hover.source"),
            ),
        })
    }

    pub(super) fn error_at(&mut self, text: &mut TextSystem, p: Point) -> Option<usize> {
        if self.link_mode
            || self.completion.is_open()
            || self.modal_open()
            || self.overlay_open()
            || self.code_hover.contains(p)
        {
            return None;
        }
        let tab = self.tabs.get_mut(self.active).filter(|t| t.editor_visible())?;
        if tab.diagnostics.marks.is_empty()
            || !tab.view.bounds().contains(p)
            || tab.view.over_scrollbar(p)
        {
            return None;
        }
        let offset = tab.view.offset_at(p, &tab.doc, &tab.folds, text, &self.palette);
        let buffer = tab.doc.buffer();
        // The mouse must be right on the text (not past the end of the line).
        let line = buffer.line_of(offset);
        if offset == buffer.line_end(line) && offset > buffer.line_start(line) {
            let last = tab
                .diagnostics
                .marks
                .iter()
                .position(|m| m.range.start >= offset && m.range.end <= offset);
            return last;
        }
        tab.diagnostics.marks.iter().position(|m| {
            let end = m.range.end.max(m.range.start + 1);
            m.range.start <= offset && offset < end
        })
    }

    /// There is a link under the mouse (the cursor becomes a hand).
    pub fn has_link(&self) -> bool {
        self.link.as_ref().is_some_and(|l| l.label.is_some())
    }

    /// Recomputes the link under the mouse (only when the mouse moves to another word).
    pub fn update_link(&mut self, text: &mut TextSystem, p: Point) -> bool {
        let had = self.has_link();
        let overlay = self.modal_open() || self.completion.is_open();
        let Some(tab) = self.tabs.get_mut(self.active).filter(|_| self.link_mode && !overlay)
        else {
            self.link = None;
            return had;
        };
        if !tab.view.bounds().contains(p) {
            self.link = None;
            return had;
        }
        let offset = tab.view.offset_at(p, &tab.doc, &tab.folds, text, &self.palette);
        let language = tab
            .syntax
            .snapshot()
            .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(offset).clone());
        let word_chars = self.support.languages.get(&language).map_or("", |l| l.word_chars);
        let range = commands::word_range(&tab.doc, offset, word_chars);
        // Only identifiers (not spaces or operators).
        let word = tab.doc.buffer().slice(range.clone());
        if range.is_empty() || !word.chars().any(|c| c.is_alphanumeric() || c == '_') {
            self.link = None;
            return had;
        }
        // A path or dotted name in quotes (`'_macros.html.twig'`, `'app.home'`) links as a
        // whole: the underline shows what the click opens.
        let word_offset = range.start + range.len() / 2;
        let intelligence = self.support.intelligence.iter().find(|i| i.language() == language);
        let sigil = intelligence.and_then(|i| i.naming().sigil);
        let range = code_intelligence::path_in_quotes(&tab.text(), offset, sigil).unwrap_or(range);
        if self.link.as_ref().is_some_and(|l| l.tab == self.active && l.range == range) {
            return false;
        }
        self.refresh_symbols();
        let current = Self::index_path(&self.tabs[self.active]);
        let label = self.definition_hint(word_offset).map(|target| {
            let line = target.line + 1;
            if target.path == current {
                i18n::tr!("link.line", line = line)
            } else {
                let name = target
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                format!("{name}:{line}")
            }
        });
        // A module's command for what's there (a console's table): its label.
        let label = label.or_else(|| {
            let source = self.tabs.get(self.active)?.text();
            match self.assist_definition(&source, word_offset)? {
                code_intelligence::AssistTarget::Command(id) => {
                    let command = self.support.commands.iter().find(|c| c.spec.id.0 == id)?;
                    Some(crate::commands::label(command.spec.id))
                }
                code_intelligence::AssistTarget::Location(_)
                | code_intelligence::AssistTarget::Choose { .. }
                | code_intelligence::AssistTarget::Code { .. } => None,
            }
        });
        // No declaration: a document link there (a URL, a file named in quotes) — the
        // server's, else ours.
        // A URL wins over a name in it (`example` of `https://example.com`).
        let link = self.document_link_at(offset);
        let url = link
            .as_ref()
            .is_some_and(|l| matches!(l.target, code_intelligence::links::LinkTarget::Url(_)));
        let (range, label, target) = match (label, link) {
            (Some(label), Some(_)) | (Some(label), None) if !url => (range, Some(label), None),
            (_, Some(link)) => {
                let label = link_label(&link.target);
                (link.range, Some(label), Some(link.target))
            }
            (label, None) => (range, label, None),
        };
        self.link = Some(DefinitionLink { tab: self.active, range, label, target });
        true
    }

    /// The document link at `offset` of the active document: the server's (asked once
    /// per version while ⌘ is held), else the built-in engine's (its line looked at).
    pub(super) fn document_link_at(
        &mut self,
        offset: usize,
    ) -> Option<code_intelligence::links::DocumentLink> {
        // Turned off in the settings: only declarations are followed.
        if !crate::insight::links() {
            return None;
        }
        let tab = self.tabs.get(self.active)?;
        let (doc, version) = (tab.doc.id(), tab.doc.version());
        let served = &self.data.served;
        let answered = served.links_answer.as_ref().filter(|(d, v, _)| *d == doc && *v == version);
        if let Some((_, _, links)) = answered
            && let Some(link) =
                links.iter().find(|l| l.range.start <= offset && offset < l.range.end)
        {
            return Some(link.clone());
        }
        let asked = served.links.as_ref().is_some_and(|(d, v, _)| *d == doc && *v == version);
        if answered.is_none()
            && !asked
            && let Some(reply) = self.ask_server(offset, |s, at| s.document_links(at))
        {
            self.data.served.links = Some((doc, version, reply));
        }
        let tab = self.tabs.get(self.active)?;
        let path = tab.doc.path()?.to_path_buf();
        let text = tab.text();
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        // What the disk said of each candidate, kept for this version of the document: the
        // mouse going back over the same words with ⌘ held asks the disk nothing again.
        let known = self.link_files.take().filter(|(of, _)| *of == (doc, version));
        let checked =
            std::cell::RefCell::new(known.unwrap_or_else(|| ((doc, version), HashMap::new())));
        let disk = &self.ports.disk;
        let exists = |p: &Path| {
            if let Some(&file) = checked.borrow().1.get(p) {
                return file;
            }
            let file = matches!(disk.kind(p), Some(k) if k != NodeKind::Directory);
            checked.borrow_mut().1.insert(p.to_path_buf(), file);
            file
        };
        let base = code_intelligence::links::LinkBase {
            file: &path,
            root: root.as_deref(),
            exists: &exists,
        };
        let link = code_intelligence::links::link_at(&text, offset, &base);
        self.link_files = Some(checked.into_inner());
        link
    }

    /// Follows a document link: a file opened (at its line), a URL in the browser.
    pub(super) fn follow_link(
        &mut self,
        text: &mut TextSystem,
        target: code_intelligence::links::LinkTarget,
    ) -> bool {
        use code_intelligence::links::LinkTarget;
        match target {
            LinkTarget::Url(url) => {
                self.request = Some(MouseResult::OpenUrl(url));
                true
            }
            LinkTarget::File { path, line } => {
                if let Err(e) = self.open_file(&path) {
                    tracing::warn!(path = %path.display(), error = %e, "can't open");
                    return false;
                }
                match line {
                    Some(line) => self.goto_position(text, line as usize, 0),
                    None => true,
                }
            }
        }
    }

    // ─── Top bar ───
}

/// What a link's badge says: a URL's address, a file's name.
fn link_label(target: &code_intelligence::links::LinkTarget) -> String {
    use code_intelligence::links::LinkTarget;
    match target {
        LinkTarget::Url(url) => {
            let shown: String = url.chars().take(60).collect();
            if shown.len() < url.len() { format!("{shown}…") } else { shown }
        }
        LinkTarget::File { path, line } => {
            let name =
                path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            match line {
                Some(line) => format!("{name}:{}", line + 1),
                None => name,
            }
        }
    }
}
