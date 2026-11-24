//! The view as the app sees it (`Viewer`): its data, the poll, the commands, the mouse,
//! the keys, the status, the paint.

use super::*;

impl Viewer for RelationsView {
    /// Its source's color (its own, else its folder's).
    fn accent(&self, palette: &ui_components::Palette) -> Option<kernel::Rgba> {
        let color = self.request.as_ref()?.access.color()?;
        ui_components::tag_colors::named(&color.name(), palette)
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<DiagramRequest>() else { return };
        self._watch = request.events.as_ref().map(|events| {
            let (flag, wake, id) =
                (Arc::clone(&self.changed), self.wake(), request.access.source());
            events.subscribe(move |e: &database::events::DataChanged| {
                if e.source == id && e.structure {
                    flag.store(true, Ordering::Release);
                    wake();
                }
            })
        });
        self.request = Some(request.clone());
        self.graph = None;
        self.history.clear();
        self.refit = true;
        self.read();
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        if self.changed.swap(false, Ordering::AcqRel) {
            self.read();
            changed = true;
        }
        if let Some(reading) = &self.reading {
            match reading.rx.try_recv() {
                Ok(result) => {
                    self.reading = None;
                    match result {
                        Ok(read) => self.show_read(read),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                    changed = true;
                }
                // Its progress is drawn as it goes.
                Err(TryRecvError::Empty) => changed = true,
                Err(TryRecvError::Disconnected) => {
                    self.reading = None;
                    changed = true;
                }
            }
        }
        if let Some((shown, graph, rx)) = self.laying.take() {
            match rx.try_recv() {
                Ok(layout) => {
                    self.take_layout(shown, graph, layout);
                    changed = true;
                }
                Err(TryRecvError::Empty) => self.laying = Some((shown, graph, rx)),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        changed
    }

    fn is_loading(&self) -> bool {
        self.busy()
    }

    fn command(&mut self, id: &str) -> bool {
        // The search has the keys: the app's editing commands are its (⌘A, ⌘Z, ⌘⇧Z, ⌘X).
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        match id {
            "fit" => {
                self.view.fit_pending = true;
                true
            }
            "back" => self.back(),
            "find" => {
                self.with_controls(|c, parts| c.focus(parts, Some(FILTER)));
                self.filter.input.select_all();
                true
            }
            "refresh" => {
                self.read();
                true
            }
            "all" => {
                self.show_all();
                true
            }
            "found" => {
                self.toggle_list();
                true
            }
            // A line by the column it starts from (`table.column`): selected, its popover
            // open.
            id if id.starts_with("edge:") => {
                let Some((table, column)) = id["edge:".len()..].split_once('.') else {
                    return false;
                };
                let graph = Arc::clone(&self.drawn);
                let link = graph.links.iter().position(|l| {
                    graph.tables.get(l.from).is_some_and(|t| t.table().name == table)
                        && l.columns.first().is_some_and(|c| c.eq_ignore_ascii_case(column))
                });
                link.is_some_and(|l| self.open_edge(l, None))
            }
            id if id.starts_with("kinds:") => match RelationKinds::parse(&id["kinds:".len()..]) {
                Some(kinds) => {
                    self.show_kinds(kinds);
                    true
                }
                None => false,
            },
            // A table by its name (`schema.name` or `name`): its neighbors, or it selected.
            id if id.starts_with("focus-table:") || id.starts_with("select-table:") => {
                let (what, name) = id.split_once(':').unwrap_or_default();
                let table = match name.split_once('.') {
                    Some((schema, name)) => TableRef::in_schema(schema, name),
                    None => TableRef::new(name),
                };
                let Some(t) = self.graph.as_ref().and_then(|g| g.find(&table)) else {
                    return false;
                };
                if what == "focus-table" {
                    self.show_neighbors(t, self.depth);
                } else {
                    self.select(t);
                }
                true
            }
            id => {
                let Some((what, t)) = id.split_once(':') else { return false };
                let Ok(t) = t.parse::<usize>() else { return false };
                match what {
                    "open" => {
                        self.pending = self.open(t);
                        self.pending.is_some()
                    }
                    "neighbors" => {
                        self.show_neighbors(t, self.depth);
                        true
                    }
                    "select" => {
                        self.select(t);
                        true
                    }
                    "depth" => match self.focus {
                        Some(f) => {
                            self.show_neighbors(f, t.clamp(1, 3));
                            true
                        }
                        None => false,
                    },
                    "accept" => self.accept(t, true),
                    "ignore" => self.accept(t, false),
                    "unaccept" => self.undo_decision(t, true),
                    "unignore" => self.undo_decision(t, false),
                    "copy" => {
                        let Some(name) = self
                            .graph
                            .as_ref()
                            .and_then(|g| g.tables.get(t))
                            .map(|t| t.table().to_string())
                        else {
                            return false;
                        };
                        self.pending = Some(ViewerAction::Request(AppRequest::CopyText(name)));
                        true
                    }
                    _ => false,
                }
            }
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.list_open
            && self.hits.iter().any(|(r, h)| *h == Hit::List && r.contains(self.pointer))
        {
            self.list_scroll = (self.list_scroll + dy / self.view.scale.max(1.0)).max(0.0);
            return true;
        }
        self.view.scroll(dx, dy);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.view.zoom_at(factor, at, ZOOM_MIN, ZOOM_MAX)
    }

    fn zoom_level(&self) -> Option<f32> {
        Some(self.view.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        self.view.reset_zoom(ZOOM_MIN, ZOOM_MAX)
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Option<ViewerAction> {
        let hit = self.hit(p);
        // A popover closes at a click outside it (as the app's popovers do); the click
        // goes on.
        if !hit.is_some_and(Hit::in_popover) {
            self.popover = None;
        }
        // The filter (its caret put): it has the keys; any other press takes them back.
        if self.with_controls(|c, parts| c.press(parts, p, clicks)).is_some() {
            return None;
        }
        // The second click of a double click: the card the first one centered opens.
        if clicks >= 2
            && let Some(t) = self.clicked.take()
        {
            self.view.press = None;
            return self.open(t);
        }
        if hit.and_then(Hit::table).is_none() {
            self.clicked = None;
        }
        // Over the list, a bar or a popover: its buttons act at release, the diagram
        // doesn't pan.
        if hit.is_some_and(|h| !h.in_diagram()) {
            self.view.press(p);
            self.view.moved = false;
            return None;
        }
        self.view.press(p);
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some((_, moved)) = self.with_controls(|c, parts| c.drag(parts, p)) {
            return moved;
        }
        // Pressed on the list or a control: no pan.
        let at = self.view.press.and_then(|at| self.hit(at));
        if at.is_some_and(|h| !h.in_diagram()) {
            return false;
        }
        self.view.drag(p)
    }

    fn release(&mut self) -> bool {
        if self.with_controls(|c, parts| c.release(parts)).is_some() {
            return false;
        }
        match self.view.release() {
            Release::None => false,
            Release::Panned => {
                self.clicked = None;
                true
            }
            Release::Click(at) => {
                let Some(hit) = self.hit(at) else {
                    // On nothing: the line selected let go.
                    return self.edge.take().is_some();
                };
                self.activate(hit, at);
                true
            }
        }
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let t = self.hit(p).and_then(Hit::table)?;
        Some(self.card_menu(t))
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        // The filter's ×: what it does.
        if self.filter_shown && Control::cursor(&self.filter, p) == Some(ViewerCursor::Pointer) {
            return Some(Tip::new(self.filter.area(), t("relations.tip_clear")));
        }
        let under = self.hit(p)?;
        self.tips
            .iter()
            .rev()
            .find(|(hit, tip)| *hit == under && tip.anchor.contains(p))
            .map(|(_, tip)| tip.clone())
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        match self.hit(p)? {
            Hit::Field => Some(ViewerCursor::Text),
            Hit::Bar | Hit::Popover | Hit::List => None,
            _ => Some(ViewerCursor::Pointer),
        }
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn wants_text(&self) -> bool {
        self.typing()
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        // The filter's keys while it has them.
        if self.typing() {
            return self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
        }
        match key {
            // Esc: the popover closed, then the line let go.
            ViewerKey::Escape => self.close_popover() || self.edge.take().is_some(),
            // Tab (⇧: back) goes through the selected table's lines; Enter opens the
            // popover of the one selected.
            ViewerKey::Tab => self.cycle_edges(reach.extend),
            ViewerKey::Enter => match self.selected_edge() {
                Some(_) if self.popover.is_some() => false,
                Some(_) => {
                    let key = self.edge.clone();
                    let link = key.and_then(|k| k.find(&self.drawn));
                    link.is_some_and(|l| self.open_edge(l, None))
                }
                None => false,
            },
            _ => false,
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, ui_components::KeyReach::default())
    }

    fn select_all(&mut self) -> bool {
        self.with_controls(|c, parts| c.command(parts, "select_all")).is_some()
    }

    fn status(&self) -> Option<String> {
        self.graph.as_ref()?;
        Some(tr!(
            "relations.status",
            tables = self.shown.tables.len(),
            links = self.shown.links.len()
        ))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(waker) = &ctx.waker {
            *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(waker));
        }
        // Only what's drawn now can be pressed.
        self.filter_shown = false;
        self.paint_view(ctx);
    }
}
