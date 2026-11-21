//! The profile as a view: what it reacts to.

use super::*;

impl Viewer for ProfileView {
    fn memory(&self) -> ViewMemory {
        ViewMemory { ram: std::mem::size_of_val(self.table.rows()) as u64, gpu: 0 }
    }

    fn actions_view(&self) -> Option<&'static str> {
        Some(crate::profile::VIEW)
    }

    fn crumbs(&self) -> Vec<String> {
        if !matches!(self.source, Source::Tab) {
            return Vec::new();
        }
        vec![
            i18n::t("console.crumb_consoles"),
            self.source_name.clone(),
            self.console_name.clone(),
            i18n::t("profile.view.crumb"),
        ]
    }

    fn accent(&self, palette: &ui_components::Palette) -> Option<Rgba> {
        if !matches!(self.source, Source::Tab) {
            return None;
        }
        ui_components::tag_colors::named(self.accent.as_deref()?, palette)
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(data) = data.downcast_ref::<ProfileTabData>() {
            // Made again for that profile, the project's sizes kept.
            let sizes = self.sizes.take();
            *self = Self::of(data.clone());
            if let Some(sizes) = sizes {
                self.take_sizes(sizes);
            }
        } else if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            self.take_sizes(sizes.clone());
        }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.paint_view(ctx);
        // What its parts want of the frames (a splitter's fade).
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.refresh();
        if let Some(pending) = &self.export {
            let done = pending.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(result) = done {
                self.export = None;
                self.notifications.push(match result {
                    Ok(path) => Notification::success(i18n::tr!(
                        "profile.view.exported",
                        path = path.display().to_string().as_str()
                    )),
                    Err(why) => Notification::error(i18n::tr!(
                        "profile.view.export_failed",
                        why = why.as_str()
                    )),
                });
                changed = true;
            }
        }
        changed
    }

    fn is_animating(&self) -> bool {
        self.controls.is_animating()
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Option<ViewerAction> {
        self.press(p, clicks)
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        match self.with_controls(|c, parts| c.press(parts, p, clicks)) {
            // The splitter: twice, the panel's width back to its own.
            Some((SPLITTER, _)) => {
                if clicks == 2 {
                    self.with_controls(|c, parts| c.release(parts));
                    self.panel_w = layout::PANEL_W;
                    if let Some(sizes) = &self.sizes {
                        sizes.remove(PANEL_SIZE);
                    }
                }
                return None;
            }
            // The filter: its caret put (it has the keys), its × emptying it.
            Some((FILTER, _)) => return None,
            // A side panel: its scrollbar's thumb held, or what its list says of the press
            // (the filter lets the keys go).
            Some(_) => {
                self.typing = false;
                if let Some(hit) = self.finding_pressed.take() {
                    self.press_finding(hit);
                }
                if let Some(hit) = self.detail_pressed.take() {
                    self.press_detail(hit);
                }
                return self.action.take();
            }
            None => {}
        }
        if let Some(hit) = self.hit_at(p) {
            if hit != Hit::Filter {
                self.typing = false;
            }
            self.press_hit(hit);
            return self.action.take();
        }
        self.typing = false;
        if let Some(compare) = self.compare.as_mut() {
            match compare.press(p, clicks) {
                Some(CompareEvent::Close) => self.compare = None,
                Some(CompareEvent::Swap) => {
                    self.compare = self.compare.take().map(CompareView::swapped);
                }
                Some(CompareEvent::Finding(i)) => {
                    // An index into the report's findings: the shown one's place.
                    if let Some(at) = self.shown.iter().position(|&f| f == i) {
                        self.choose_finding(at);
                    }
                }
                Some(CompareEvent::Step(node)) => self.select_node(Some(node)),
                None => {}
            }
            return self.action.take();
        }
        if self.body.contains(p) {
            match self.mode {
                ProfileMode::Graph => {
                    let (_, event) = self.graph.press(p, clicks, self.selected_node());
                    if let Some(event) = event {
                        self.graph_event(event);
                    }
                }
                ProfileMode::Flame => {
                    if let Some(node) = self.flame.press(p) {
                        self.select_node(Some(node));
                    }
                }
                ProfileMode::Tables => {
                    self.tables.press(p, clicks);
                    self.tables_chosen();
                }
                ProfileMode::Tree => {
                    self.table.press(p, clicks);
                    // A step chosen: its finding chosen too (or none).
                    self.follow_choice();
                }
            }
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        let held = self.with_controls(|c, parts| c.drag(parts, p));
        if let (Some((SPLITTER, _)), Some(center)) = (held, self.split_at) {
            let w = ((self.bounds.max_x() - center) / self.scale.max(0.01)).max(layout::PANEL_MIN);
            if w != self.panel_w {
                self.panel_w = w;
                // The project's: every profile view of it (and the next session) as wide.
                if let Some(sizes) = &self.sizes {
                    sizes.set(PANEL_SIZE, w.round());
                }
                return true;
            }
            return false;
        }
        if let Some(compare) = self.compare.as_mut() {
            return compare.table.drag(p);
        }
        let body = match self.mode {
            ProfileMode::Graph => self.graph.drag(p),
            ProfileMode::Tables => self.tables.drag(p),
            _ => self.table.drag(p),
        };
        held.is_some_and(|(_, moved)| moved) || body
    }

    fn release(&mut self) -> bool {
        let graph = match self.mode {
            ProfileMode::Graph => match self.graph.release() {
                Some(event) => {
                    self.graph_event(event);
                    true
                }
                None => false,
            },
            _ => false,
        };
        let others =
            self.compare.as_mut().is_some_and(|c| c.table.release()) | self.tables.release();
        let parts = self.with_controls(|c, parts| c.release(parts));
        parts.is_some_and(|(_, changed)| changed) | self.table.release() | graph | others
    }

    fn hover(&mut self, p: Point) -> bool {
        let hit = self.hit_at(p);
        let mut changed = hit != self.hover;
        self.hover = hit;
        changed |= self.with_controls(|c, parts| c.hover(parts, Some(p)));
        let inside = self.body.contains(p);
        if let Some(compare) = self.compare.as_mut() {
            let over = inside || self.laid.panel.contains(p);
            let body = if over { compare.hover(p) } else { compare.leave() };
            return changed || body;
        }
        let body = match (self.mode, inside) {
            (ProfileMode::Graph, true) => self.graph.hover(p),
            (ProfileMode::Graph, false) => self.graph.leave(),
            (ProfileMode::Flame, true) => self.flame.hover(p),
            (ProfileMode::Flame, false) => self.flame.icicle.leave(),
            (ProfileMode::Tables, true) => self.tables.hover(p),
            (ProfileMode::Tables, false) => self.tables.leave(),
            (_, true) => self.table.hover(p),
            (_, false) => self.table.leave(),
        };
        changed || body
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if let Some(compare) = self.compare.as_mut() {
            return compare.scroll(dx, dy);
        }
        // Over a side panel: its list's (never through it to the body under a drawer).
        if self.with_controls(|c, parts| c.scroll_hovered(parts, dx, dy)).is_some() {
            return self.panel_scrolled;
        }
        match self.mode {
            ProfileMode::Graph => self.graph.scroll(dx, dy),
            ProfileMode::Flame => self.flame.scroll(dy),
            ProfileMode::Tables => self.tables.scroll(dx, dy),
            _ => self.table.scroll(dx, dy),
        }
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // Its parts' (the splitter's, a finding's card).
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        match self.hit_at(p) {
            Some(Hit::Filter) => return Some(ViewerCursor::Text),
            Some(Hit::Statement | Hit::Depth | Hit::Engine | Hit::RolledBack | Hit::Pill(_)) => {
                return None;
            }
            Some(_) => return Some(ViewerCursor::Pointer),
            None => {}
        }
        if let Some(compare) = &self.compare {
            if compare.is_over_button(p) {
                return Some(ViewerCursor::Pointer);
            }
            return compare.table.cursor_at(p);
        }
        match self.mode {
            ProfileMode::Graph => (self.graph.is_control(p) || self.graph.is_over_card(p))
                .then_some(ViewerCursor::Pointer),
            ProfileMode::Flame => {
                self.flame.tooltip_at(p).is_some().then_some(ViewerCursor::Pointer)
            }
            ProfileMode::Tables => self.tables.cursor_at(p),
            _ => self.table.cursor_at(p),
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        if let Some((r, _, tip)) = self.hits.iter().rev().find(|(r, ..)| r.contains(p)) {
            return Some(Tip::new(*r, tip.clone()?));
        }
        if let Some(compare) = &self.compare {
            return compare.tooltip_at(p);
        }
        // A side panel's (a drawer's over the body: none of the body's through it).
        if let Some(tip) = self.controls.tooltip_at(p) {
            return Some(tip);
        }
        if self.body.contains(p) && !self.laid.panel.contains(p) {
            return match self.mode {
                ProfileMode::Graph => self.graph.tooltip_at(p),
                ProfileMode::Flame => self.flame.tooltip_at(p),
                ProfileMode::Tables => self.tables.tooltip_at(p),
                _ => self.table.tooltip_at(p),
            };
        }
        None
    }

    fn take_menu(&mut self) -> Option<(Rect, Vec<ViewMenuEntry>)> {
        self.asked_menu.take()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if !self.body.contains(p) {
            return None;
        }
        let mut entries = Vec::new();
        if self.compare.is_some() {
            if let Some(compare) = self.compare.as_mut()
                && let Some(CompareEvent::Step(node)) = compare.press(p, 1)
            {
                self.select_node(Some(node));
            }
            entries.push(
                ViewMenuEntry::item("compare.copy", i18n::t("profile.compare.copy"))
                    .with_icon("copy"),
            );
            entries.push(
                ViewMenuEntry::item("compare.swap", i18n::t("profile.compare.swap"))
                    .with_icon("transpose"),
            );
            entries.push(
                ViewMenuEntry::item("compare.close", i18n::t("profile.compare.close"))
                    .with_icon("x"),
            );
            return Some(entries);
        }
        match self.mode {
            ProfileMode::Graph => {
                if let Some(node) = self.graph.node_at(p) {
                    self.select_node(Some(node));
                }
                entries.push(
                    ViewMenuEntry::item("graph.fit", i18n::t("profile.view.graph_fit"))
                        .with_icon("maximize-2")
                        .with_shortcut("0"),
                );
                if self.selected_node().is_some() {
                    entries.push(
                        ViewMenuEntry::item("graph.center", i18n::t("profile.view.graph_center"))
                            .with_icon("crosshair"),
                    );
                }
                let flow = match self.graph.flow() {
                    ui_components::diagram::Flow::Down => "profile.view.graph_flow_right",
                    ui_components::diagram::Flow::Right => "profile.view.graph_flow_down",
                };
                entries
                    .push(ViewMenuEntry::item("graph.flow", i18n::t(flow)).with_icon("rotate-ccw"));
                entries.push(ViewMenuEntry::Separator);
            }
            ProfileMode::Flame => {
                if let Some(node) = self.flame.node_at(p) {
                    self.select_node(Some(node));
                }
                if self.selected_node().is_some() {
                    entries.push(
                        ViewMenuEntry::item("flame.zoom", i18n::t("profile.view.flame_zoom"))
                            .with_icon("maximize-2"),
                    );
                }
                if self.flame.icicle.focus().is_some() {
                    entries.push(
                        ViewMenuEntry::item("flame.back", i18n::t("profile.view.flame_back"))
                            .with_icon("arrow-left"),
                    );
                    entries
                        .push(ViewMenuEntry::item("flame.all", i18n::t("profile.view.flame_all")));
                }
                entries.push(ViewMenuEntry::Separator);
            }
            ProfileMode::Tables => {
                self.tables.press(p, 1);
                self.tables_chosen();
                if self.tables.selected().is_some() {
                    entries.push(
                        ViewMenuEntry::item("tables.open", i18n::t("profile.tables.open"))
                            .with_icon("table"),
                    );
                    entries.push(
                        ViewMenuEntry::item(
                            "tables.structure",
                            i18n::t("profile.tables.structure"),
                        )
                        .with_icon("column"),
                    );
                    entries.push(
                        ViewMenuEntry::item("tables.copy_row", i18n::t("console.plan_copy_row"))
                            .with_icon("copy"),
                    );
                    entries.push(ViewMenuEntry::Separator);
                }
            }
            ProfileMode::Tree => {
                self.table.press(p, 1);
                self.follow_choice();
            }
        }
        if self.table.selected().is_some() && self.mode != ProfileMode::Tables {
            entries.push(
                ViewMenuEntry::item("plan.copy_row", i18n::t("console.plan_copy_row"))
                    .with_icon("copy"),
            );
        }
        entries.push(
            ViewMenuEntry::item("profile.export:text", ExportKind::Text.label())
                .with_icon("clipboard"),
        );
        entries.push(ViewMenuEntry::Separator);
        entries.push(
            ViewMenuEntry::item("profile.next_problem", i18n::t("profile.view.next_problem"))
                .with_icon("alert")
                .with_shortcut("F2"),
        );
        entries.push(
            ViewMenuEntry::item("profile.prev_problem", i18n::t("profile.view.prev_problem"))
                .with_shortcut("⇧F2"),
        );
        entries.push(
            ViewMenuEntry::item("profile.hottest", i18n::t("profile.view.hottest"))
                .with_icon("flame")
                .with_shortcut("H"),
        );
        entries.push(ViewMenuEntry::Separator);
        entries.push(
            ViewMenuEntry::item("plan.expand_all", i18n::t("console.plan_expand_all"))
                .with_icon("chevrons-expand"),
        );
        entries.push(
            ViewMenuEntry::item("plan.collapse_all", i18n::t("console.plan_collapse_all"))
                .with_icon("chevrons-collapse"),
        );
        Some(entries)
    }

    fn selected_text(&self) -> Option<String> {
        if self.typing
            && let Some(selected) = Control::selected_text(&self.filter)
        {
            return Some(selected);
        }
        self.table.row_text(self.table.selected()?)
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.mode == ProfileMode::Graph && self.graph.zoom_by(factor, Some(at))
    }

    fn zoom_level(&self) -> Option<f32> {
        (self.mode == ProfileMode::Graph).then_some(self.graph.view.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        self.mode == ProfileMode::Graph
            && self
                .graph
                .view
                .reset_zoom(crate::profile::graph::ZOOM_MIN, crate::profile::graph::ZOOM_MAX)
    }

    fn wants_text(&self) -> bool {
        self.typing
    }

    fn text_input(&mut self, text: &str) -> bool {
        if !self.typing {
            let mut chars = text.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => self.key(c),
                _ => false,
            };
        }
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.typing && self.with_controls(|c, parts| c.paste(parts, text)).is_some()
    }

    fn select_all(&mut self) -> bool {
        self.typing && self.with_controls(|c, parts| c.command(parts, "select_all")).is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The filter's keys while it has them: what it doesn't use stays its own, but the
        // arrows and the pages, which give the keys back to the steps.
        if self.typing {
            let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
            if used || self.typing {
                return true;
            }
        }
        if let Some(compare) = self.compare.as_mut() {
            if key == ViewerKey::Escape {
                self.compare = None;
                return true;
            }
            let used = compare.key(key);
            if let Some(node) = compare.table.selected().and_then(|r| compare.own_step(r))
                && Some(node) != self.selected_node()
            {
                self.select_node(Some(node));
            }
            return used;
        }
        if key == ViewerKey::Escape {
            if self.drawer_open && !self.laid.mode_wide {
                self.drawer_open = false;
                return true;
            }
            if !self.filter.text().is_empty() {
                self.set_filter("");
                return true;
            }
            return false;
        }
        match self.mode {
            ProfileMode::Graph => {
                let report = self.report().cloned();
                let selected = self.selected_node();
                match (key, selected, report) {
                    (ViewerKey::Enter, Some(node), _) => {
                        self.graph_event(GraphEvent::Toggle(node));
                        true
                    }
                    (_, None, Some(report)) => {
                        let first = report.profile.roots.first().copied();
                        self.select_node(first);
                        first.is_some()
                    }
                    (_, Some(node), Some(report)) => {
                        match GraphView::step_from(&report, node, key, self.graph.flow()) {
                            Some(next) => {
                                self.select_node(Some(next));
                                true
                            }
                            None => false,
                        }
                    }
                    _ => false,
                }
            }
            ProfileMode::Tables => {
                let used = self.tables.key(key);
                if used {
                    self.tables_chosen();
                }
                used
            }
            ProfileMode::Flame => {
                let used = self.flame.key(key);
                let chosen = self.flame.selected();
                if chosen != self.selected_node() {
                    self.select_node(chosen);
                }
                used
            }
            _ => {
                let used = self.table.key(key);
                if used {
                    // The details follow the choice.
                    self.details_for = None;
                }
                used
            }
        }
    }

    fn key(&mut self, key: char) -> bool {
        if key == '*' {
            return self.table.char_key('*');
        }
        // The diagram's zoom: + / − and 0 (fit).
        if self.mode == ProfileMode::Graph {
            match key {
                '+' | '=' => return self.graph.zoom_by(1.25, None),
                '-' | '_' => return self.graph.zoom_by(0.8, None),
                '0' => {
                    let selected = self.selected_node();
                    self.graph.fit(selected);
                    return true;
                }
                _ => {}
            }
        }
        // Typing starts the filter (the part having the keys).
        self.with_controls(|c, parts| c.char_key(parts, key)).is_some()
    }

    fn command(&mut self, id: &str) -> bool {
        // The filter has the keys: the app's editing commands are its (⌘A, ⌘Z, ⌘X).
        if self.typing && self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        match id {
            "profile.next_problem" | "next_problem" => return self.next_problem(true),
            "profile.prev_problem" | "prev_problem" => return self.next_problem(false),
            "profile.hottest" | "hottest" => return self.hottest(),
            "plan.copy" | "copy_text" => {
                if let Some(r) = self.report().cloned() {
                    self.copy(export::text(&r), "profile.view.copied_text");
                }
            }
            "copy" | "plan.copy_row" => {
                let list =
                    if self.mode == ProfileMode::Tables { &self.tables } else { &self.table };
                let Some(text) = list.selected().and_then(|s| list.row_text(s)) else {
                    return false;
                };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
            }
            "plan.expand_all" | "profile.expand_all" | "expand_all" => {
                self.table.set_all_open(true)
            }
            "plan.collapse_all" | "profile.collapse_all" | "collapse_all" => {
                self.table.set_all_open(false)
            }
            "profile.open_in_tab" | "open_in_tab" => self.open_in_tab(),
            "view_tree" => {
                self.set_mode(ProfileMode::Tree);
            }
            "view_graph" => {
                self.set_mode(ProfileMode::Graph);
            }
            "view_flame" => {
                self.set_mode(ProfileMode::Flame);
            }
            "view_tables" => {
                self.set_mode(ProfileMode::Tables);
            }
            "compare" => {
                let anchor = self
                    .hits
                    .iter()
                    .find(|(_, h, _)| *h == Hit::Compare)
                    .map(|(r, ..)| *r)
                    .unwrap_or_default();
                let items = self.compare_items();
                self.open_dropdown(anchor, items);
            }
            "compare.close" => return self.close_compare(),
            "compare.swap" => {
                if self.compare.is_none() {
                    return false;
                }
                self.compare = self.compare.take().map(CompareView::swapped);
            }
            "compare.copy" => {
                let Some(text) = self.compare.as_mut().map(CompareView::text) else {
                    return false;
                };
                self.copy(text, "profile.compare.copied");
            }
            id if id.starts_with("profile.compare:") => {
                let Some(other) =
                    database::profile::ProfileId::parse(&id["profile.compare:".len()..])
                else {
                    return false;
                };
                return self.compare_with(other);
            }
            "tables.open" => self.open_chosen_table(false),
            "tables.structure" => self.open_chosen_table(true),
            "tables.copy_row" => {
                let Some(text) = self.tables.selected().and_then(|s| self.tables.row_text(s))
                else {
                    return false;
                };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
            }
            "graph.fit" => {
                let selected = self.selected_node();
                self.graph.fit(selected);
            }
            "graph.center" => {
                let Some(node) = self.selected_node() else { return false };
                return self.graph.center_on(node);
            }
            "graph.flow" => self.graph.turn(),
            "flame.back" => return self.flame.icicle.back(),
            "flame.all" => return self.flame.icicle.zoom_to(None),
            "flame.zoom" => {
                let Some(node) = self.selected_node() else { return false };
                return self.flame.icicle.zoom_to(Some(node.index()));
            }
            "export" => {
                let anchor = self
                    .hits
                    .iter()
                    .find(|(_, h, _)| *h == Hit::Export)
                    .map(|(r, ..)| *r)
                    .unwrap_or_default();
                let items = self.export_items();
                self.open_dropdown(anchor, items);
            }
            id if id.starts_with("profile.export:") => {
                let Some(kind) = ExportKind::from_id(&id["profile.export:".len()..]) else {
                    return false;
                };
                self.export(kind);
            }
            id if id.starts_with("profile.mode:") => {
                let mode =
                    ProfileMode::ALL.into_iter().find(|m| m.id() == &id["profile.mode:".len()..]);
                if !mode.is_some_and(|m| self.set_mode(m)) {
                    return false;
                }
            }
            id if id.starts_with("picked-save:") => {
                self.write_json(PathBuf::from(&id["picked-save:".len()..]));
            }
            id if id.starts_with("finding:") => {
                let Ok(i) = id["finding:".len()..].parse::<usize>() else { return false };
                self.choose_finding(i);
            }
            id if id.starts_with("insert:") => {
                let Ok(i) = id["insert:".len()..].parse::<usize>() else { return false };
                self.insert(i);
            }
            id if id.starts_with("ignore:") => {
                let rule = id["ignore:".len()..].to_owned();
                if let Some(runner) = &self.runner {
                    runner.ignore_rule(&rule, true);
                }
            }
            id if id.starts_with("severity:") => {
                let l = Level::ALL
                    .into_iter()
                    .find(|l| format!("{l:?}").to_lowercase() == id["severity:".len()..]);
                self.severity = if self.severity == l { None } else { l };
                self.shown_generation += 1;
            }
            "drawer" => self.drawer_open = !self.drawer_open,
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn status(&self) -> Option<String> {
        let set = self.set.as_ref()?;
        let report = set.report.as_ref()?;
        Some(i18n::tr!(
            "console.plan_summary",
            count = report.profile.nodes.len(),
            time = millis(set.elapsed)
        ))
    }
}

/// The splitter's and the filter's places among the view's parts.
const SPLITTER: usize = 0;
const FILTER: usize = 3;

impl ProfileView {
    /// Everything it shows, its parts placed.
    fn paint_view(&mut self, mut ctx: ViewerPaint<'_>) {
        self.refresh();
        let b = ctx.bounds;
        self.bounds = b;
        self.scale = ctx.scale;
        self.hits.clear();
        self.splitter.set_style(SplitterStyle::from_palette(ctx.palette));
        let compact_rows = if self.report().is_some() { self.listed().len() } else { 0 };
        let spec = LayoutSpec {
            bounds: b,
            scale: ctx.scale,
            panel_w: self.panel_w,
            drawer_open: self.drawer_open,
            compact_rows,
        };
        let key = (
            b.min_x().to_bits(),
            b.min_y().to_bits(),
            b.size.width.to_bits(),
            b.size.height.to_bits(),
            ctx.scale.to_bits(),
            self.panel_w.to_bits(),
            self.drawer_open,
            compact_rows,
        );
        self.laid = *self.layout.get_or_update(key, || layout::layout(&spec));
        let laid = self.laid;
        self.paint_bar(&mut ctx, laid.bar);
        let Some(report) = self.report().cloned() else {
            let ds = Ctx::new(ctx.palette, ctx.scale);
            let body = Rect::new(
                b.min_x(),
                laid.bar.max_y(),
                b.size.width,
                (b.max_y() - laid.bar.max_y()).max(0.0),
            );
            let layer = ctx.scene.push_layer(Some(body));
            layer.quad(Quad::filled(body, ctx.palette.editor_background));
            ds::empty_state(layer, ctx.text, &ds, body, &i18n::t("console.result_gone"), None);
            return;
        };
        self.paint_summary(&mut ctx, laid.summary, &report);
        if laid.compact.size.height > 0.0 {
            self.paint_compact(&mut ctx, laid.compact);
        }
        // The columns that fit the tree's room, its words first.
        let tree_logical = laid.tree.size.width / ctx.scale.max(0.01);
        let words = self.table.tree_width(ctx.text, ctx.palette, ctx.scale);
        let gutter = if self.table.rows().iter().any(|r| r.sign.is_some()) {
            ui_components::tree_table::GUTTER
        } else {
            0.0
        };
        let fit = tree::fitting(&self.columns_all, tree_logical - gutter - 12.0, words);
        self.rebuild(&report, Some(fit));
        let selected = self.selected_node();
        // The comparison and the tables take the panel's room too when it's beside (their
        // tables are wide); in a drawer the panel still opens over them.
        let whole = if laid.mode_wide {
            Rect::new(
                laid.tree.min_x(),
                laid.tree.min_y(),
                laid.panel.max_x() - laid.tree.min_x(),
                laid.tree.size.height,
            )
        } else {
            laid.tree
        };
        self.body = laid.tree;
        let own_room = self.compare.is_some() || self.mode == ProfileMode::Tables;
        if own_room {
            self.body = whole;
            if let Some(compare) = self.compare.as_mut() {
                compare.paint(whole, ctx.scale, ctx.text, ctx.scene, ctx.palette, ctx.icons);
            } else {
                self.paint_tables(&mut ctx, whole, &report);
            }
            if laid.drawer && laid.panel.size.width > 0.0 && self.compare.is_none() {
                self.paint_panel(&mut ctx, laid.panel, &report);
            } else {
                self.findings_area = Rect::default();
                self.details_area = Rect::default();
            }
            // No splitter: nothing beside.
            self.splitter.set_bounds(Rect::default(), ctx.scale);
            return;
        }
        match self.mode {
            ProfileMode::Tables => {}
            ProfileMode::Graph => {
                let findings = shown_of(&report, &self.shown);
                self.graph.paint(crate::profile::graph::GraphPaint {
                    area: laid.tree,
                    scale: ctx.scale,
                    text: ctx.text,
                    scene: ctx.scene,
                    palette: ctx.palette,
                    icons: ctx.icons,
                    report: &report,
                    findings: &findings,
                    table: &mut self.table,
                    row_of: &self.built.row_of,
                    selected,
                });
            }
            ProfileMode::Flame => {
                let findings = shown_of(&report, &self.shown);
                self.flame.sync(&report, &findings, self.shown_generation);
                let painter = ui_components::Painter {
                    text: ctx.text,
                    scene: ctx.scene,
                    palette: ctx.palette,
                    icons: ctx.icons,
                };
                self.flame.paint(laid.tree, ctx.scale, painter, selected);
            }
            ProfileMode::Tree => self.table.paint(TablePaint {
                bounds: laid.tree,
                scale: ctx.scale,
                text: ctx.text,
                scene: ctx.scene,
                palette: ctx.palette,
                icons: ctx.icons,
            }),
        }
        if laid.panel.size.width > 0.0 {
            self.paint_panel(&mut ctx, laid.panel, &report);
        } else {
            self.findings_area = Rect::default();
            self.details_area = Rect::default();
        }
        // The splitter after both panes.
        self.splitter.set_bounds(laid.split, ctx.scale);
        self.splitter.paint(ctx.scene);
    }

    /// What a press on the findings' list hit.
    fn press_finding(&mut self, hit: FindingHit) {
        let listed = self.listed();
        match hit {
            FindingHit::Card(i) | FindingHit::Step(i) => {
                if let Some(&f) = listed.get(i) {
                    self.choose_finding(f);
                }
            }
            FindingHit::Copy(i) => {
                let sql = listed.get(i).and_then(|&f| {
                    self.shown_findings().get(f)?.suggestion.as_ref()?.sql().map(str::to_owned)
                });
                if let Some(sql) = sql {
                    self.copy(sql, "profile.view.copied_sql");
                }
            }
            FindingHit::Insert(i) => {
                if let Some(&f) = listed.get(i) {
                    self.insert(f);
                }
            }
            FindingHit::Ignore(i) => {
                let rule =
                    listed.get(i).and_then(|&f| self.shown_findings().get(f).map(|f| f.rule));
                if let (Some(rule), Some(runner)) = (rule, &self.runner) {
                    runner.ignore_rule(rule, true);
                    self.selected_finding = None;
                    self.notifications.push(Notification::info(i18n::t("profile.view.ignored")));
                }
            }
            FindingHit::ShowIgnored => {
                if let Some(runner) = &self.runner {
                    runner.unignore_all();
                }
            }
        }
    }

    /// What a press on a step's details hit.
    fn press_detail(&mut self, hit: DetailHit) {
        match hit {
            DetailHit::Value(v) if !v.is_empty() => self.copy(v, "profile.view.copied_value"),
            DetailHit::Problem(i) => self.choose_finding(i),
            _ => {}
        }
    }

    /// Its parts (the splitter, the findings' and the details' panels while they're drawn),
    /// the topmost first, with their state of use. The body's views (the tree, the graph,
    /// the flame graph, the tables, the comparison: one at a time, by its mode) and the
    /// filter stay its own.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let (mut findings_scrolled, mut details_scrolled, mut edited) = (false, false, false);
        let mut splitter = Divider { splitter: &mut self.splitter, at: &mut self.split_at };
        let mut findings = Panel {
            rows: &mut self.findings,
            area: self.findings_area,
            pressed: &mut self.finding_pressed,
            scrolled: &mut findings_scrolled,
        };
        let mut details = Panel {
            rows: &mut self.details,
            area: self.details_area,
            pressed: &mut self.detail_pressed,
            scrolled: &mut details_scrolled,
        };
        let mut filter =
            FilterPart { field: &mut self.filter, typing: &mut self.typing, edited: &mut edited };
        let mut parts: [&mut dyn Control; 4] =
            [&mut splitter, &mut findings, &mut details, &mut filter];
        // The filter has the keys without a press (nothing else takes them).
        self.controls.focus(&mut parts, Some(FILTER));
        let answer = f(&mut self.controls, &mut parts);
        self.panel_scrolled = findings_scrolled || details_scrolled;
        if edited {
            self.filtered();
        }
        answer
    }
}

/// The steps' filter as the part having the keys: the app's search field (its press, drag,
/// editing keys, ⌘A / ⌘X / ⌘Z, × emptying it) while it's `typing`; before that, a character
/// typed with nothing else to take it starts it. Esc empties it and gives the keys back, ↵
/// gives them back, the arrows and the pages give them back to the steps. What it changed
/// is said back (`edited`; the view filters).
struct FilterPart<'a> {
    field: &'a mut SearchField,
    typing: &'a mut bool,
    edited: &'a mut bool,
}

impl FilterPart<'_> {
    /// Runs `edit` on the field, noting whether its text changed.
    fn editing(&mut self, edit: impl FnOnce(&mut SearchField) -> bool) -> bool {
        let used = edit(&mut *self.field);
        *self.edited |= self.field.take_edited();
        used
    }
}

impl Control for FilterPart<'_> {
    /// Where it was drawn as a field (none while it's an icon alone).
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.field)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let press = Control::press(self.field, p, clicks);
        // Its × emptied it and let the keys go; elsewhere its caret is put.
        let cleared = self.field.take_cleared();
        *self.typing = !cleared;
        *self.edited |= cleared | self.field.take_edited();
        press
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Focused from code only: Tab doesn't stop at it.
    fn in_tab_order(&self) -> bool {
        false
    }

    /// Tab while it's typed into stays its own (nothing else to go to).
    fn keeps_tab(&self) -> bool {
        *self.typing
    }

    /// Any but a control character or a space starts it.
    fn char_key(&mut self, key: char) -> bool {
        if key.is_control() || key == ' ' {
            return false;
        }
        *self.typing = true;
        let mut buf = [0; 4];
        let typed: &str = key.encode_utf8(&mut buf);
        self.editing(|f| Control::text(f, typed))
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if !*self.typing {
            return false;
        }
        match key {
            ViewerKey::Escape => {
                *self.edited |= !SearchField::text(self.field).is_empty();
                self.field.set_text("");
                *self.typing = false;
            }
            ViewerKey::Enter => *self.typing = false,
            ViewerKey::Up | ViewerKey::Down | ViewerKey::PageUp | ViewerKey::PageDown => {
                *self.typing = false;
                return false;
            }
            _ => return self.editing(|f| Control::key(f, key, reach)),
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        *self.typing && self.editing(|f| Control::text(f, typed))
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field).filter(|_| *self.typing)
    }

    fn command(&mut self, id: &str) -> bool {
        *self.typing && self.editing(|f| Control::command(f, id))
    }
}

/// What a side panel's list (the findings' cards, a step's details) answers as a part of the
/// view.
trait PanelRows {
    /// What a press on it hits.
    type Hit;
    fn thumb(&mut self) -> Thumb<'_>;
    fn hit_at(&self, p: Point) -> Option<Self::Hit>;
    /// The mouse at `p` (anywhere: off it, nothing lit): `true` if what it shows changed.
    fn hover_at(&mut self, p: Point) -> bool;
    fn scroll_by(&mut self, dy: f32) -> bool;
    fn tip_at(&self, p: Point) -> Option<Tip>;
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor>;
}

impl PanelRows for FindingsList {
    type Hit = FindingHit;
    fn thumb(&mut self) -> Thumb<'_> {
        FindingsList::thumb(self)
    }
    fn hit_at(&self, p: Point) -> Option<FindingHit> {
        self.hit(p)
    }
    fn hover_at(&mut self, p: Point) -> bool {
        self.hover(p)
    }
    fn scroll_by(&mut self, dy: f32) -> bool {
        FindingsList::scroll_by(self, dy)
    }
    fn tip_at(&self, p: Point) -> Option<Tip> {
        self.tooltip_at(p)
    }
    /// A card and its buttons are pressed.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.hit(p).map(|_| ViewerCursor::Pointer)
    }
}

impl PanelRows for DetailsList {
    type Hit = DetailHit;
    fn thumb(&mut self) -> Thumb<'_> {
        DetailsList::thumb(self)
    }
    fn hit_at(&self, p: Point) -> Option<DetailHit> {
        self.hit(p)
    }
    fn hover_at(&mut self, p: Point) -> bool {
        self.hover(p)
    }
    fn scroll_by(&mut self, dy: f32) -> bool {
        DetailsList::scroll_by(self, dy)
    }
    fn tip_at(&self, p: Point) -> Option<Tip> {
        self.tooltip_at(p)
    }
    /// Its rows are copied by a press, without a hand over them.
    fn cursor_at(&self, _p: Point) -> Option<ViewerCursor> {
        None
    }
}

/// A side panel as a part of the view, there while it's drawn: a press on its scrollbar
/// holds the thumb, elsewhere on it what its list hit is kept for the view; the wheel over
/// it is its list's (`scrolled`: it moved), never the body's under it.
struct Panel<'a, L: PanelRows> {
    rows: &'a mut L,
    /// Where it was drawn this frame (empty: not drawn).
    area: Rect,
    pressed: &'a mut Option<L::Hit>,
    scrolled: &'a mut bool,
}

impl<L: PanelRows> Control for Panel<'_, L> {
    fn area(&self) -> Option<Rect> {
        Some(self.area).filter(|a| a.size.width > 0.0 && a.size.height > 0.0)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let mut thumb = self.rows.thumb();
        if thumb.hit(p) && thumb.press(p, clicks) == Press::Held {
            return Press::Held;
        }
        *self.pressed = self.rows.hit_at(p);
        Press::Took
    }

    fn drag(&mut self, p: Point) -> bool {
        self.rows.thumb().drag(p)
    }

    fn release(&mut self) -> bool {
        self.rows.thumb().release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let bar = self.rows.thumb().hover(p);
        bar | self.rows.hover_at(p.unwrap_or(NOWHERE))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.rows.cursor_at(p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        *self.scrolled |= self.rows.scroll_by(dy);
        true
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.rows.tip_at(p)
    }
}
