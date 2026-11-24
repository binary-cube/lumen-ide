//! A press and what follows it: a block's bar or button, a checkbox, a fold, a link or a
//! diagram acted on; a selection started (by words, by blocks) and extended, the document
//! scrolled while it's dragged past an edge; the text's menu.

use super::*;

impl MarkdownViewer {
    /// A press (`clicks` in a row): a code block's bar or buttons, a checkbox, a fold, a
    /// failing diagram's line act at once; elsewhere a selection starts (by words on a
    /// double click, by blocks on a triple; ⇧ extends the one there is) — on a link or a
    /// diagram, followed at the release if the mouse didn't move.
    pub(super) fn press_at(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        if !self.bounds.contains(p) {
            return None;
        }
        // The bars take the press first: the document's lane, then a code block's across
        // (the one under the press); a thumb dragged (or jumped to).
        let (before, local) = (self.hover, self.local(p));
        self.hover = Some(local);
        match self.with_controls(|c, parts| c.press(parts, p, clicks)) {
            Some((DOCUMENT_BAR, _)) => {
                self.hover = before;
                self.scroll = self.scroll.clamp(0.0, self.max_scroll());
                return None;
            }
            Some(_) => return None,
            None => {}
        }
        if let Some((i, button)) = self.copy_button()
            && button.contains(local)
        {
            let code = self.layout.as_ref()?.code_blocks[i].1.clone();
            self.notifications
                .push(workbench_model::Notification::success(i18n::t("markdown.code_copied")));
            return Some(ViewerAction::Request(AppRequest::CopyText(code)));
        }
        if let Some((i, button)) = self.zoom_button()
            && button.contains(local)
        {
            return self.open_diagram(i);
        }
        let hit = self.hit_at(local);
        match hit {
            None | Some(Hit::Link(_) | Hit::Diagram(_)) => {
                self.start_selection(p, clicks, mods, hit);
                None
            }
            Some(hit) => self.act(hit),
        }
    }

    /// What a click on `hit` does.
    fn act(&mut self, hit: Hit) -> Option<ViewerAction> {
        match hit {
            Hit::Link(url) => self.open_link(&url),
            Hit::Task(range) => {
                let text = toggle_task(&self.source, range)?;
                Some(ViewerAction::SetText { path: self.path.clone(), text })
            }
            Hit::Fold(index) => {
                if !self.toggled.remove(&index) {
                    self.toggled.insert(index);
                }
                self.dirty = true;
                None
            }
            Hit::Line(line) => {
                Some(ViewerAction::OpenAt { path: self.path.clone(), line: line as u32 })
            }
            Hit::Diagram(index) => self.open_diagram(index),
        }
    }

    /// A selection starts at `p` (window coordinates).
    fn start_selection(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
        hit: Option<Hit>,
    ) {
        let Some(at) = self.offset_at(self.local(p)) else { return };
        let Some(layout) = &self.layout else { return };
        let runs = &layout.runs;
        let (grain, selection) = match clicks {
            2 => {
                let (a, b) = select::word_at(runs, at);
                (Grain::Word(a, b), Selection { anchor: a, focus: b })
            }
            3.. => {
                let (a, b) = select::block_at(runs, at);
                (Grain::Block(a, b), Selection { anchor: a, focus: b })
            }
            _ => match self.selection {
                Some(s) if mods.shift => (Grain::Char, Selection { anchor: s.anchor, focus: at }),
                _ => (Grain::Char, Selection::at(at)),
            },
        };
        let pending = hit.filter(|_| clicks == 1 && !mods.shift).map(|h| (p, h));
        self.selection = Some(selection);
        self.selecting = Some(Selecting { grain, pending, last: p });
    }

    /// The selection grown to the mouse at `p` (window coordinates; above or below the
    /// view: what's there, out of sight), by its grain. `true` if it changed.
    pub(super) fn extend_to(&mut self, p: Point) -> bool {
        let Some(grain) = self.selecting.as_ref().map(|s| s.grain) else { return false };
        let b = self.bounds;
        let p = Point::new(p.x.clamp(b.min_x(), b.max_x()), p.y);
        let Some(at) = self.offset_at(self.local(p)) else { return false };
        let Some(layout) = &self.layout else { return false };
        let old = self.selection.unwrap_or_default();
        let grow = |(a, b): (usize, usize), (wa, wb): (usize, usize)| {
            if wa < a {
                Selection { anchor: b, focus: wa }
            } else {
                Selection { anchor: a, focus: wb.max(b) }
            }
        };
        let new = match grain {
            Grain::Char => Selection { anchor: old.anchor, focus: at },
            Grain::Word(a, b) => grow((a, b), select::word_at(&layout.runs, at)),
            Grain::Block(a, b) => grow((a, b), select::block_at(&layout.runs, at)),
        };
        let changed = self.selection != Some(new);
        self.selection = Some(new);
        changed
    }

    /// The right-click menu over the text: Copy and Copy as Markdown (with something
    /// selected), Select All.
    pub(super) fn text_menu(&self) -> Vec<ui_components::ViewMenuEntry> {
        use ui_components::ViewMenuEntry;
        let keys = |id: &str, entry: ViewMenuEntry| match ui_components::shortcuts::label(id) {
            Some(k) => entry.with_shortcut(k),
            None => entry,
        };
        let mut menu = Vec::new();
        if self.selected().is_some() {
            let copy =
                ViewMenuEntry::item("markdown.copy", i18n::t("markdown.copy")).with_icon("copy");
            menu.push(keys("edit.copy", copy));
            menu.push(ViewMenuEntry::item(
                "markdown.copy_markdown",
                i18n::t("markdown.copy_markdown"),
            ));
            menu.push(ViewMenuEntry::Separator);
        }
        let all = ViewMenuEntry::item("markdown.select_all", i18n::t("markdown.select_all"));
        menu.push(keys("edit.select_all", all));
        menu
    }

    /// The offset under `local` (document coordinates).
    pub(super) fn offset_at(&self, local: Point) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let shift = |b: usize| self.code_shift(b);
        Some(select::offset_at(&layout.runs, local, &shift))
    }

    /// How far scrolling code block `block` is moved across.
    pub(super) fn code_shift(&self, block: usize) -> f32 {
        self.code_scroll.get(&block).copied().unwrap_or(0.0).round()
    }

    /// The selection's offsets, if it holds anything (within the layout's text).
    pub(super) fn selected(&self) -> Option<(usize, usize)> {
        let total = select::total(&self.layout.as_ref()?.runs);
        let (a, b) = self.selection?.range();
        let (a, b) = (a.min(total), b.min(total));
        (a < b).then_some((a, b))
    }

    /// Selecting with the mouse near the top or bottom (or past them): how fast the
    /// document scrolls there (physical pixels a second, up when negative).
    pub(super) fn edge_speed(&self) -> Option<f32> {
        let sel = self.selecting.as_ref().filter(|s| s.pending.is_none())?;
        let s = self.layout.as_ref().map_or(1.0, |l| l.scale);
        let (b, y, edge) = (self.bounds, sel.last.y, EDGE * s);
        let depth = if y < b.min_y() + edge {
            -(b.min_y() + edge - y)
        } else if y > b.max_y() - edge {
            y - (b.max_y() - edge)
        } else {
            return None;
        };
        let speed = (depth * 14.0).clamp(-6000.0 * s, 6000.0 * s);
        let room = if speed < 0.0 { self.scroll } else { self.max_scroll() - self.scroll };
        (room > 0.0).then_some(speed)
    }

    /// A frame while a drag holds the mouse near an edge: the document scrolls on, the
    /// selection with it.
    pub(super) fn edge_scroll(&mut self) {
        let Some(speed) = self.edge_speed() else {
            self.edge_tick = None;
            return;
        };
        let now = std::time::Instant::now();
        let dt =
            self.edge_tick.map_or(1.0 / 60.0, |t| now.duration_since(t).as_secs_f32().min(0.1));
        self.edge_tick = Some(now);
        self.scroll = (self.scroll + speed * dt).clamp(0.0, self.max_scroll());
        if let Some(last) = self.selecting.as_ref().map(|s| s.last) {
            self.extend_to(last);
        }
    }
}
