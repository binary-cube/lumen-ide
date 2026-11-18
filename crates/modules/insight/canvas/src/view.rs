//! The canvas as a view: what it reacts to (clicks, drags, keys, scrolling, zoom) and what it shows
//! each frame.

use super::*;

impl Viewer for Canvas {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        // Opened (again): the project as it is now, centered on the active file.
        if let Some(start) = data.downcast_ref::<CanvasStart>() {
            self.builder.set_queries(Arc::clone(&start.queries));
            if let Some((path, line)) = start.focus.clone() {
                self.builder.request(path, line);
            }
            return;
        }
        if let Some(graph) = data.downcast_ref::<Arc<DepGraph>>() {
            self.show(Arc::clone(graph));
        }
    }

    fn poll(&mut self) -> bool {
        // The edit applied meanwhile: built again with the new code.
        if let Some((path, line)) = self.rebuild.take() {
            self.builder.request(path, line);
        }
        match self.builder.poll() {
            Some(graph) => {
                self.show(Arc::new(graph));
                true
            }
            None => false,
        }
    }

    fn is_loading(&self) -> bool {
        self.builder.is_building()
    }

    fn wants_text(&self) -> bool {
        self.search.is_some() || self.editing.is_some()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        if let Some(search) = &mut self.search {
            let catalog = self.graph.as_ref().map(|g| g.catalog.as_slice()).unwrap_or(&[]);
            search.insert(typed, catalog);
            return true;
        }
        if let Some(edit) = &mut self.editing {
            edit.insert(typed);
            self.relayout();
            return true;
        }
        false
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if let Some(search) = &mut self.search {
            match key {
                ViewerKey::Escape => self.search = None,
                ViewerKey::Backspace => {
                    let catalog = self.graph.as_ref().map(|g| g.catalog.as_slice()).unwrap_or(&[]);
                    search.backspace(catalog);
                }
                ViewerKey::Left | ViewerKey::Right | ViewerKey::Home | ViewerKey::End => {
                    search.all = false;
                }
                ViewerKey::Up => search.step(-1),
                ViewerKey::Down | ViewerKey::Tab => search.step(1),
                ViewerKey::Enter => {
                    let i = search.results.get(search.selected).copied();
                    self.pending = i.and_then(|i| self.jump(i));
                    self.search = None;
                }
                _ => return false,
            }
            return true;
        }
        if let Some(edit) = &mut self.editing {
            if key == ViewerKey::Escape {
                self.finish_edit();
                return true;
            }
            let used = edit.key(key);
            self.relayout();
            return used;
        }
        false
    }

    fn command(&mut self, id: &str) -> bool {
        if let Some(used) = self.action(id) {
            return used;
        }
        // ⌘A in search: selects all text.
        if id == "select_all" {
            return match &mut self.search {
                Some(search) => {
                    search.all = !search.query.is_empty();
                    true
                }
                None => false,
            };
        }
        // `edit`: editing the selected declaration (like a click in its code).
        if id == "edit"
            && let (Some(graph), Some(i)) = (self.graph.clone(), self.selected)
        {
            let f = &graph.files[graph.focus];
            let s = &f.symbols[i];
            self.editing = Some(edit::CodeEdit::new(i, f.absolute.clone(), s.line, &s.code));
            self.relayout();
            return true;
        }
        if id != "search" || self.graph.is_none() {
            return false;
        }
        self.finish_edit();
        self.search = Some(search::Search::default());
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        let action = self.pending.take();
        // An edit: once the app applied it, the graph is built again there.
        if let Some(ViewerAction::Replace { path, lines, .. }) = &action {
            self.rebuild = Some((path.clone(), Some(*lines.start())));
        }
        action
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.builder.set_waker(ctx.waker.clone());
        self.bounds = ctx.bounds;
        self.scale = ctx.scale;
        if self.fit_pending && self.layout.is_some() {
            self.fit();
        }
        self.hits.clear();
        let palette = ctx.palette;
        let ds = Ctx::new(palette, ctx.scale);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        // Background: slightly darker than the editor, with a dot grid.
        layer.quad(Quad::filled(b, palette.window_background));
        let k = self.zoom * self.scale;
        let origin =
            Point::new(self.bounds.min_x() - self.pan.x * k, self.bounds.min_y() - self.pan.y * k);
        ui_components::diagram::dot_grid(layer, palette, b, origin, self.scale);
        let Some(graph) = self.graph.clone() else {
            let font = ds.ui_font(0.0);
            let shaped = ctx.text.shape_line(&t("canvas.loading"), &font);
            let mid = self.center();
            draw_text(
                layer,
                &shaped,
                (mid.x - shaped.width / 2.0).round(),
                mid.y.round(),
                palette.text_muted,
            );
            return;
        };
        let Some(layout) = self.layout.take() else { return };
        self.paint_graph(&graph, &layout, &ds, ctx.text, ctx.scene, ctx.icons);
        self.layout = Some(layout);
        self.paint_toolbar(&ds, ctx.text, ctx.scene.push_layer(Some(b)));
        self.paint_search(&graph, &ds, ctx.text, ctx.scene, ctx.icons);
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let k = self.zoom * self.scale;
        self.pan = Point::new(self.pan.x + dx / k, self.pan.y + dy / k);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.zoom_at(factor, at)
    }

    fn zoom_level(&self) -> Option<f32> {
        Some(self.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        self.zoom_at(1.0 / self.zoom, self.center())
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press = Some(p);
        let graph = self.graph.clone()?;
        let hit = self.hit(p);
        // A click outside the search closes it; outside the edited code, it ends editing.
        if self.search.is_some()
            && !matches!(hit, Some(Hit::SearchField | Hit::SearchClear | Hit::SearchResult(_)))
        {
            self.search = None;
        }
        if self.editing.is_some() && hit != Some(Hit::Edit) {
            self.finish_edit();
            if let Some(action) = self.pending.take() {
                self.press = None;
                return Some(action);
            }
        }
        let hit = hit?;
        self.press = None;
        match hit {
            Hit::Card(kind) => {
                self.moving = Some((kind, p, false));
                None
            }
            Hit::Edit => {
                if self.editing.is_none()
                    && let Some(i) = self.selected
                {
                    let f = &graph.files[graph.focus];
                    let s = &f.symbols[i];
                    self.editing =
                        Some(edit::CodeEdit::new(i, f.absolute.clone(), s.line, &s.code));
                    self.relayout();
                }
                if let Some(edit) = &mut self.editing {
                    edit.click(p);
                }
                None
            }
            Hit::SearchField => {
                match &mut self.search {
                    Some(search) => search.all = false,
                    None => self.search = Some(search::Search::default()),
                }
                None
            }
            Hit::SearchClear => {
                match &mut self.search {
                    Some(search) => search.clear(),
                    None => self.search = Some(search::Search::default()),
                }
                None
            }
            Hit::SearchResult(i) => {
                let entry = self.search.as_ref().and_then(|s| s.results.get(i).copied());
                self.search = None;
                entry.and_then(|e| self.jump(e))
            }
            Hit::Mode(mode) => {
                self.mode = mode;
                self.relayout();
                self.fit_pending = true;
                None
            }
            Hit::ZoomIn => {
                self.zoom_at(1.2, self.center());
                None
            }
            Hit::ZoomOut => {
                self.zoom_at(1.0 / 1.2, self.center());
                None
            }
            Hit::Fit => {
                self.fit();
                None
            }
            Hit::File(i) => {
                let path = graph.files[i].absolute.clone();
                self.builder.request(path, None);
                None
            }
            Hit::FocusHeader => Some(ViewerAction::OpenAt {
                path: graph.files[graph.focus].absolute.clone(),
                line: 0,
            }),
            Hit::Symbol(i) => {
                self.selected = Some(i);
                self.relayout();
                None
            }
            Hit::Open { line } => {
                Some(ViewerAction::OpenAt { path: graph.files[graph.focus].absolute.clone(), line })
            }
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some((kind, from, moved)) = &mut self.moving {
            let k = self.zoom * self.scale;
            let (dx, dy) = ((p.x - from.x) / k, (p.y - from.y) / k);
            if !*moved && dx.hypot(dy) * k < 3.0 {
                return false;
            }
            *moved = true;
            *from = p;
            let kind = *kind;
            let offset = self.offsets.entry(kind).or_insert(Point::new(0.0, 0.0));
            *offset = Point::new(offset.x + dx, offset.y + dy);
            // Only this card and its edge ends move (no full relayout per mouse move).
            if let Some(layout) = &mut self.layout {
                layout.shift(&HashMap::from([(kind, Point::new(dx, dy))]));
            }
            return true;
        }
        let Some(from) = self.press else { return false };
        let k = self.zoom * self.scale;
        self.pan = Point::new(self.pan.x - (p.x - from.x) / k, self.pan.y - (p.y - from.y) / k);
        self.press = Some(p);
        true
    }

    fn release(&mut self) -> bool {
        // A click (without movement) on a declaration's code card selects it.
        if let Some((kind, _, moved)) = self.moving.take() {
            if !moved
                && let CardKind::Code { symbol } = kind
                && self.selected != Some(symbol)
            {
                self.selected = Some(symbol);
                self.relayout();
            }
            return true;
        }
        self.press.take().is_some()
    }

    /// A hand over a card, a row, a button.
    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        self.hit(p).map(|_| ui_components::ViewerCursor::Pointer)
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        // A redraw only when what's under the mouse changed.
        let now = self.hit(p);
        std::mem::replace(&mut self.hover, now.clone()) != now
    }

    fn status(&self) -> Option<String> {
        let graph = self.graph.as_ref()?;
        Some(format!(
            "{} · {}%",
            graph.files[graph.focus].path.display(),
            (self.zoom * 100.0).round()
        ))
    }
}
