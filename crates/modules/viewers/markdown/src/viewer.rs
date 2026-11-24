//! The preview as a view (`Viewer`): its memory, the source followed, the paint, the mouse,
//! the selection, the source's line kept in step, the menu, the commands.

use super::*;

impl Viewer for MarkdownViewer {
    /// Its source and its images.
    fn memory(&self) -> ui_components::ViewMemory {
        let (ram, gpu) = self.drawn.memory();
        let ram = ram + self.caches.as_ref().map_or(self.caches_ram, layout::Caches::memory);
        // The selectable text: each run's place and text.
        let runs = self.layout.as_ref().map_or(0, |l| {
            l.runs
                .iter()
                .map(|r| std::mem::size_of_val(r) + r.text.capacity() + r.copy.capacity())
                .sum::<usize>()
        });
        ui_components::ViewMemory { ram: ram + (self.source.capacity() + runs) as u64, gpu }
    }

    fn purge(&mut self) {
        // The layout holds the images too: it goes as well (rebuilt on the next paint). A
        // build on its way is let go, with the caches it took.
        self.drawn.clear();
        match &mut self.caches {
            Some(caches) => caches.clear(),
            None => self.caches = Some(layout::Caches::default()),
        }
        self.building = None;
        self.layout_text = None;
        self.layout = None;
    }

    fn set_source(&mut self, source: &str) {
        if source != self.source {
            self.source = source.to_owned();
            self.version += 1;
        }
    }

    fn moved(&mut self, path: &Path) {
        self.path = path.to_path_buf();
        self.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    }

    /// Its images are still being decoded (the app redraws until they're in).
    fn is_loading(&self) -> bool {
        self.building.is_some()
            || self.caches.as_ref().is_some_and(layout::Caches::loading)
            || self.drawn.loading()
            || self.export.as_ref().is_some_and(view::DiagramView::waiting)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.bounds = ctx.bounds;
        let palette = ctx.palette;
        let width = ctx.bounds.size.width;
        self.waker.clone_from(&ctx.waker);
        self.drawn.set_waker(ctx.waker.clone());
        self.drawn.poll();
        self.take_built();
        if let Some(caches) = &mut self.caches {
            caches.set_waker(ctx.waker.clone());
            if caches.poll() {
                self.dirty = true;
            }
        }
        let wrap = settings::wrap_code();
        let stale = self.dirty
            || self.layout.as_ref().is_none_or(|l| {
                l.version != self.version
                    || l.width != width
                    || l.scale != ctx.scale
                    || l.wrap != wrap
                    || self.shows_lost_image(l, ctx.bounds)
            })
            || self.built_with.as_ref() != Some(palette);
        // Made only when something is to be built (the palette is copied into it).
        if stale && self.building.is_none() {
            let version = self.version;
            let want =
                build::Want { version, width, scale: ctx.scale, wrap, palette: palette.clone() };
            if self.failed.as_ref() != Some(&want) {
                self.build(want, ctx.text);
            }
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        self.edge_scroll();
        let copy = self.copy_button();
        let selected = self.selected();
        let Some(layout) = &self.layout else {
            self.with_controls(|c, parts| c.settle(parts));
            return;
        };
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, palette.editor_background));
        // Whole pixels, so text and boxes scroll in step (no jitter).
        let dy = b.min_y() - self.scroll.round();
        let visible = |r: &Rect| r.max_y() + dy >= b.min_y() && r.min_y() + dy <= b.max_y();
        let offset = |r: &Rect| {
            Rect::new(r.min_x() + b.min_x(), r.min_y() + dy, r.size.width, r.size.height)
        };
        // The selection, under everything it covers (a formula's glyphs, a diagram's shapes,
        // the text; what's tinted over it lets it show).
        let boxes = selected.map_or_else(Vec::new, |(from, to)| {
            select::boxes(&layout.runs, from, to, |r| visible(r))
        });
        for (rect, _, _) in boxes.iter().filter(|(_, scroll, _)| scroll.is_none()) {
            layer.quad(Quad::filled(offset(rect), palette.selection));
        }
        for item in &layout.items {
            match item {
                Item::Rect { rect, color, radius } if visible(rect) => {
                    layer.quad(Quad::filled(offset(rect), *color).rounded(*radius));
                }
                Item::Border { rect, color, width, radius } if visible(rect) => {
                    layer.quad(
                        Quad::filled(offset(rect), kernel::Rgba::TRANSPARENT)
                            .rounded(*radius)
                            .bordered(*width, *color),
                    );
                }
                Item::Image { rect, image } if visible(rect) => {
                    layer.image(image, offset(rect));
                    budget::drawn(image);
                }
                Item::Picture { rect, path, stamp, decode } => {
                    // Decoded a screen ahead; drawn when it's in view.
                    let margin = b.size.height;
                    let near = Rect::new(
                        rect.min_x(),
                        rect.min_y() - margin,
                        rect.size.width,
                        rect.size.height + 2.0 * margin,
                    );
                    if visible(&near)
                        && let Some(image) =
                            self.drawn.images.image(path, *stamp, *decode, ctx.scale)
                        && visible(rect)
                    {
                        layer.image(&image, offset(rect));
                        budget::drawn(&image);
                    }
                }
                Item::Diagram { rect, scene, k, key } => {
                    // The tiles in view, and those a screen away made ahead.
                    let margin = b.size.height;
                    let near = Rect::new(
                        rect.min_x(),
                        rect.min_y() - margin,
                        rect.size.width,
                        rect.size.height + 2.0 * margin,
                    );
                    if visible(&near) {
                        let at = offset(rect);
                        let placement = tiles::Placement {
                            origin: Point::new(at.min_x(), at.min_y()),
                            stretch: 1.0,
                            view: b,
                            margin,
                            fetch: true,
                        };
                        tiles::draw(layer, &mut self.drawn.tiles, scene, *k, *key, placement);
                    }
                }
                _ => {}
            }
        }
        for item in &layout.items {
            match item {
                Item::Text { x, baseline, text, font, color } => {
                    let y = baseline + dy;
                    if y + font.size < b.min_y() || y - font.size > b.max_y() {
                        continue;
                    }
                    let shaped = ctx.text.shape_line(text, font);
                    draw_text(layer, &shaped, (x + b.min_x()).round(), y.round(), *color);
                }
                Item::Icon { rect, name, color } if visible(rect) => {
                    if let Some(id) = ctx.icons.id(name) {
                        draw_icon(layer, ctx.icons, id, offset(rect), *color);
                    }
                }
                _ => {}
            }
        }
        // Code blocks that scroll across: their text clipped to their frame, moved by its
        // own scroll.
        for item in &layout.items {
            let Item::Scroll { view, block, content, room, items } = item else { continue };
            if !visible(view) {
                continue;
            }
            let clip = offset(view);
            let shift = self
                .code_scroll
                .get(block)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, (content - room).max(0.0))
                .round();
            let inner = ctx.scene.push_layer(Some(clip.intersect(&b).unwrap_or(clip)));
            for (rect, _, _) in boxes.iter().filter(|(_, scroll, _)| *scroll == Some(*block)) {
                let r = offset(rect);
                let r = Rect::new(r.min_x() - shift, r.min_y(), r.size.width, r.size.height);
                inner.quad(Quad::filled(r, palette.selection));
            }
            for it in items {
                let Item::Text { x, baseline, text, font, color } = it else { continue };
                let y = baseline + dy;
                if y + font.size < b.min_y() || y - font.size > b.max_y() {
                    continue;
                }
                let shaped = ctx.text.shape_line(text, font);
                draw_text(inner, &shaped, (x - shift + b.min_x()).round(), y.round(), *color);
            }
        }
        // The hovered block's bar across (or the one being dragged).
        if let Some((block, view, metrics)) = self.hscroll_block() {
            let v = offset(&view);
            // The whole track: the thumb widened on hover fits it (a thinner one cut its
            // rounded ends); at rest it sits by the edge as before.
            let thickness = ui_components::Scrollbar::track_thickness(ctx.scale);
            let track = Rect::new(v.min_x(), v.max_y() - thickness, v.size.width, thickness);
            let bar = self.hbar.get_or_insert_with(|| {
                ui_components::Scrollbar::new(
                    ui_components::ScrollbarStyle::from_palette(palette),
                    ui_components::Orientation::Horizontal,
                )
            });
            bar.set_style(ui_components::ScrollbarStyle::from_palette(palette));
            bar.set_track(track, ctx.scale);
            bar.set_expansion(if bar.is_hovered() || bar.is_dragging() { 1.0 } else { 0.0 });
            bar.paint(ctx.scene, metrics);
            self.hbar_block = Some(block);
        } else if !self.hbar.as_ref().is_some_and(ui_components::Scrollbar::is_dragging) {
            self.hbar_block = None;
        }
        let layer = ctx.scene.push_layer(Some(b));
        // What sits over scrolled text (a code block's language).
        for item in &layout.items {
            let Item::Badge { pill, bg, text } = item else { continue };
            if !visible(pill) {
                continue;
            }
            layer.quad(Quad::filled(offset(pill), *bg).rounded((4.0 * ctx.scale).round()));
            if let Item::Text { x, baseline, text, font, color } = &**text {
                let shaped = ctx.text.shape_line(text, font);
                draw_text(layer, &shaped, (x + b.min_x()).round(), (baseline + dy).round(), *color);
            }
        }
        // The hovered code block's copy button, over its language; a diagram's "open
        // larger".
        let buttons = copy
            .map(|(_, b)| (b, "copy"))
            .into_iter()
            .chain(self.zoom_button().map(|(_, b)| (b, "maximize-2")));
        let ds = ds::Ctx::new(palette, ctx.scale);
        for (button, icon) in buttons {
            let over = self.hover.is_some_and(|h| button.contains(h));
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Raised)
                .icon(icon)
                .icon_size(FLOATING_ICON)
                .rest_tone(ds::Tone::Neutral)
                .hovered(over);
            ds::button(layer, ctx.text, ctx.icons, &ds, offset(&button), spec);
        }
        // Over the previews' budget: what's been out of sight longest goes.
        budget::frame_done(budget::GPU_BUDGET);
        // The scrollbar (only when the document is taller than the view).
        let (height, scroll) = (layout.height, self.scroll);
        self.vbar.paint(ctx.scene, palette, ctx.scale, b, height, scroll);
        // What the bars want of the frames, for the questions that come before the next one.
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        // Mostly across, over a code block wider than itself: that block scrolls.
        if dx.abs() > dy.abs()
            && let Some((block, _, m)) = self.hscroll_block()
        {
            let to = (m.offset + dx).clamp(0.0, (m.content - m.viewport).max(0.0));
            self.code_scroll.insert(block, to);
            return true;
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
        self.scroll != before
    }

    fn drag(&mut self, p: Point) -> bool {
        // A bar's thumb held: the document, or a code block across.
        let before = self.scroll;
        if let Some((part, moved)) = self.with_controls(|c, parts| c.drag(parts, p)) {
            if part != DOCUMENT_BAR {
                return moved;
            }
            self.scroll = self.scroll.clamp(0.0, self.max_scroll());
            return self.scroll != before;
        }
        if let Some(sel) = &mut self.selecting {
            sel.last = p;
            // A press on a link or a diagram: followed unless the mouse moves away.
            if let Some((from, _)) = &sel.pending {
                let s = self.layout.as_ref().map_or(1.0, |l| l.scale);
                if (p.x - from.x).hypot(p.y - from.y) < 4.0 * s {
                    return false;
                }
                sel.pending = None;
            }
            return self.extend_to(p);
        }
        false
    }

    fn release(&mut self) -> bool {
        let released = self.with_controls(|c, parts| c.release(parts));
        if released == Some((DOCUMENT_BAR, true)) {
            return true;
        }
        self.edge_tick = None;
        if let Some(sel) = self.selecting.take() {
            if let Some((_, hit)) = sel.pending {
                self.selection = None;
                self.action = match hit {
                    Hit::Link(url) => self.open_link(&url),
                    Hit::Diagram(index) => self.open_diagram(index),
                    _ => None,
                };
            }
            if self.selection.is_some_and(|s| s.is_empty()) {
                self.selection = None;
            }
            return true;
        }
        released.is_some_and(|(_, changed)| changed)
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press_at(p, 1, ui_components::PressMods::default())
    }

    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.press_at(p, clicks, mods)
    }

    /// A drag selecting near an edge scrolls on its own; a bar moving.
    fn is_animating(&self) -> bool {
        self.edge_speed().is_some() || self.controls.is_animating()
    }

    /// A bar fading in or out: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        if !self.bounds.contains(p) {
            return None;
        }
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        let local = self.local(p);
        if self.hit_at(local).is_some() {
            return None;
        }
        let layout = self.layout.as_ref()?;
        let shift = |b: usize| self.code_shift(b);
        let over = layout.runs.iter().any(|r| {
            r.shaped.is_some() && {
                let x = local.x + r.scroll.map_or(0.0, shift);
                r.rect.contains(Point::new(x, local.y))
            }
        });
        over.then_some(ui_components::ViewerCursor::Text)
    }

    fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selected()?;
        let text = select::text(&self.layout.as_ref()?.runs, a, b);
        (!text.is_empty()).then_some(text)
    }

    fn select_all(&mut self) -> bool {
        let Some(layout) = &self.layout else { return false };
        self.selection = Some(Selection { anchor: 0, focus: select::total(&layout.runs) });
        true
    }

    fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    fn selects_in_split(&self) -> bool {
        true
    }

    fn source_line_at_top(&self) -> Option<f32> {
        if !settings::sync_scroll() {
            return None;
        }
        if let Some(line) = self.sync_to {
            return Some(line);
        }
        Some(self.layout.as_ref()?.lines.line_at(self.scroll))
    }

    fn scroll_to_source(&mut self, line: f32) -> bool {
        if !settings::sync_scroll() {
            return false;
        }
        match &self.layout {
            Some(l) if l.version == self.version => {
                let before = self.scroll;
                self.scroll = l.lines.y_of(line).clamp(0.0, self.max_scroll());
                (self.scroll - before).abs() > 0.5
            }
            // Not laid out yet (or for an older text): there once it is.
            _ => {
                self.sync_to = Some(line);
                true
            }
        }
    }

    fn reveal_source_line(&mut self, line: f32) -> bool {
        if !settings::sync_scroll() {
            return false;
        }
        let Some(l) = self.layout.as_ref().filter(|l| l.version == self.version) else {
            return false;
        };
        let y = l.lines.y_of(line);
        let h = self.bounds.size.height;
        if y >= self.scroll && y <= self.scroll + h * 0.9 {
            return false;
        }
        self.scroll = (y - h / 3.0).clamp(0.0, self.max_scroll());
        true
    }
    fn hover(&mut self, p: Point) -> bool {
        if !self.bounds.contains(p) {
            self.hover = None;
            return false;
        }
        let local = self.local(p);
        self.hover = Some(local);
        // The bars widen under it (what's lit below says the redraw).
        self.with_controls(|c, parts| c.hover(parts, Some(p)));
        self.hit_at(local).is_some() || self.copy_button().is_some_and(|(_, b)| b.contains(local))
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        let local = self.local(p);
        let on_selection = self
            .selected()
            .is_some_and(|(a, b)| self.offset_at(local).is_some_and(|at| at > a && at < b));
        let diagram = self.layout.as_ref()?.diagrams.iter().position(|d| d.rect.contains(local));
        let Some(index) = diagram.filter(|_| !on_selection) else {
            self.menu_diagram = None;
            return Some(self.text_menu());
        };
        self.menu_diagram = Some(index);
        let mut menu = vec![
            ui_components::ViewMenuEntry::item("diagram.open", i18n::t("markdown.diagram_open"))
                .with_icon("maximize-2"),
            ui_components::ViewMenuEntry::Separator,
        ];
        menu.extend(view::menu());
        Some(menu)
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "copy" | "markdown.copy" => {
                let Some(text) = self.selected_text() else { return false };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
                return true;
            }
            "markdown.copy_markdown" => {
                let Some((a, b)) = self.selected() else { return false };
                let layout = self.layout.as_ref();
                let Some(text) = layout.and_then(|l| select::markdown(&l.runs, &self.source, a, b))
                else {
                    return false;
                };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
                return true;
            }
            "select_all" | "markdown.select_all" => return self.select_all(),
            _ => {}
        }
        // Opened larger: the one whose menu it was, else the first (asked by name).
        if id == "diagram.open" {
            self.action = self.open_diagram(self.menu_diagram.unwrap_or(0));
            return self.action.is_some();
        }
        let Some(index) = self.menu_diagram else { return false };
        if let Some(path) = id.strip_prefix("picked-save:") {
            let Some(export) = &mut self.export else { return false };
            let done = export.command(&format!("picked-save:{path}"));
            self.settle();
            return done;
        }
        let Some(request) = self.diagram_request(index) else { return false };
        let Some(palette) = &self.built_with else { return false };
        let look = mermaid::Look::of(palette);
        let mut export = view::for_preview(request, look);
        export.set_waker(self.waker.clone());
        let done = export.run(id);
        self.export = Some(export);
        self.settle();
        done
    }

    fn poll(&mut self) -> bool {
        let before = self.action.is_some();
        self.settle();
        self.action.is_some() != before
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.settle();
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn status(&self) -> Option<String> {
        // Counted once per text (the status bar asks at each frame).
        let words = match self.words.get() {
            Some((version, words)) if version == self.version => words,
            _ => {
                let words = self.source.split_whitespace().count();
                self.words.set(Some((self.version, words)));
                words
            }
        };
        Some(i18n::tr!("markdown.words", count = words))
    }
}
