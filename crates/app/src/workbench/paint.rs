//! Painting the screen and the editor groups.

use super::*;
use ui_workbench::Painter;

impl Workbench {
    /// Everything into `scene` (the snapshots', the tests').
    #[cfg_attr(not(any(test, feature = "snapshot")), allow(dead_code))]
    pub fn paint(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        self.paint_with_floating(text, scene, None);
    }

    /// [`Self::paint`], what floats (the overlays, the tooltips) drawn into `floating` when
    /// given: the window's layer over the pages, drawn over them.
    pub fn paint_with_floating(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        floating: Option<&mut Scene>,
    ) {
        // This window's caret (its theme's: windows can have different ones).
        ui_kit::caret::configure(self.caret_look());
        // The installed PHP (…) found in the background since: the levels take it.
        self.refresh_levels_if_warmed();
        self.paint_phases.mark("levels");
        self.sync_before_paint(text);
        // A side panel narrower than its header needs (a width kept from before, its title
        // known since the last frame): laid out again, wide enough.
        // Once for each width they need (a window too narrow to give it isn't laid out again
        // at every frame).
        let needs: Vec<f32> = self.tools.sides.iter().map(|s| s.header.min_width()).collect();
        let narrow = self
            .tools
            .sides
            .iter()
            .any(|side| side.header.min_width() * self.scale > side.bounds.size.width + 0.5);
        if narrow && needs != self.laid_for_headers {
            self.laid_for_headers = needs;
            self.layout();
        }
        // The views told whether they're visible (the Task Manager samples only then).
        self.sync_shown();
        self.sync_text_options();
        self.paint_phases.mark("shown");
        // A view drawing the whole window (a full-window scene): nothing under it is drawn.
        if self.modal_view_covers() {
            // The theme's effects too: suspended until it closes.
            self.suspend_theme_decorations();
            // Only what's over it (a toast, a dialog it asked for): the settings, About or
            // the projects under it aren't drawn either.
            let at = Overlay::ALL.iter().position(|o| *o == Overlay::ModalView).unwrap_or(0);
            for o in Overlay::ALL[..=at].iter().rev() {
                o.slot().paint(self, text, scene);
            }
            self.save_session_soon();
            return;
        }
        // What a module asks of the region being worked in (Focus mode): the theme's
        // effects stand still meanwhile, if it says so.
        let emphasis = self.emphasis_request();
        self.chrome.decorations.paused = emphasis.is_some_and(|e| e.pause_effects);
        let keyboard_in_overlay = self.paint_frame(text, scene);
        self.paint_phases.mark("frame");
        if self.tabs.is_empty() && self.editors.groups.len() <= 1 {
            self.paint_empty_state(text, scene);
        } else {
            self.paint_groups(text, scene);
        }
        self.paint_phases.mark("editors");
        self.paint_side_panels(text, scene, keyboard_in_overlay);
        self.paint_phases.mark("side_panels");
        if self.tool_window_shown() {
            self.paint_tool_window(text, scene);
        }
        self.paint_phases.mark("tool_window");
        self.paint_splitters(scene);
        self.paint_status(text, scene);
        self.paint_phases.mark("status");
        self.completion.paint(text, scene, &self.palette);
        self.paint_top_bar(text, scene);
        self.paint_phases.mark("top_bar");
        // The pages a window is open over: their pictures in their place, under it.
        self.paint_page_pictures(scene);
        // The content muted but where the work is; the overlays over it never are.
        self.emphasize(scene, emphasis);
        // The overlays, bottom up: the stack's order, the one input follows — on the layer
        // over the pages when there's one.
        let top = match floating {
            Some(floating) => floating,
            None => scene,
        };
        for o in Overlay::ALL.into_iter().rev() {
            o.slot().paint(self, text, top);
        }
        // A tab being dragged (its pill, where it would go), an editor's or a bottom
        // window's: over everything — the side panels, the separators, the pages.
        self.paint_tab_drag(text, top);
        self.paint_tool_tab_drop(text, top);
        let window = Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32);
        self.tooltips.paint(text, top, &self.palette, self.scale, window);
        self.save_session_soon();
        self.release_unseen_syntax();
        self.paint_phases.mark("overlays");
    }

    /// What must be up to date before a frame: a pending jump, the syntax, the version
    /// control, the symbols, the hover, the index and the side panels.
    fn sync_before_paint(&mut self, text: &mut TextSystem) {
        if let Some((line, column)) = self.pending_goto.take() {
            self.goto_position(text, line as usize, column as usize);
        }
        self.go_to_pending_offset(text);
        self.paint_phases.mark("jump");
        self.poll_insight();
        self.paint_phases.mark("insight");
        self.show_waited_actions();
        self.run_chosen_action(text);
        self.paint_phases.mark("actions");
        self.sync_syntax();
        self.paint_phases.mark("syntax");
        self.sync_decorations();
        self.paint_phases.mark("decorations");
        self.poll_symbols();
        self.paint_phases.mark("symbols");
        // The project's files as its language servers should have them, and what they said.
        self.announce_documents();
        self.paint_phases.mark("announce");
        self.use_server_answers(text);
        self.paint_phases.mark("server_answers");
        self.refresh_assisted_completion(text);
        self.paint_phases.mark("completion");
        self.resolve_code_hover();
        self.paint_phases.mark("hover");
        self.release_idle_index();
        self.paint_phases.mark("idle_index");
        self.refresh_symbols_after_parse();
        self.paint_phases.mark("symbols_after_parse");
        self.sync_side_panel();
        self.paint_phases.mark("side_panel_sync");
    }

    /// The window's frame: the tint, the tool strips and the editor's card. Returns whether
    /// a modal / an overlay's field has the keyboard.
    fn paint_frame(&mut self, text: &mut TextSystem, scene: &mut Scene) -> bool {
        let window = Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32);
        let name = self
            .project
            .as_ref()
            .and_then(|p| p.root_path().file_name())
            .map(|n| n.to_string_lossy());
        self.chrome.tint.paint(window, name.as_deref(), self.scale, scene, &self.palette);
        // The theme's decorations: over the window's background, behind everything else.
        self.paint_theme_decorations(scene);
        let palette = &self.palette;
        // The registered tool windows: at the bottom of the left strip (Git last, in its
        // corner);
        // on the right strip.
        let mut buttons: Vec<ToolButton> = self
            .docked(ui_workbench::Dock::Left)
            .iter()
            .map(|t| self.tool_button_of(t, false))
            .collect();
        buttons.extend(
            self.docked(ui_workbench::Dock::Bottom).iter().map(|t| self.tool_button_of(t, true)),
        );
        self.tools.strip.paint(&buttons, &self.icons, text, palette, scene);
        // The right strip: its top group, then those asking for its bottom (the Task Manager).
        let panels: Vec<ToolButton> = self
            .docked(ui_workbench::Dock::Right)
            .iter()
            .map(|t| self.tool_button_of(t, t.strip_end()))
            .collect();
        self.tools.right_strip.paint(&panels, &self.icons, text, palette, scene);
        let keyboard_in_overlay = self.keyboard_in_overlay();
        let card = scene.push_layer(None);
        let line = self.scale.round().max(1.0);
        // Effects shown behind the editor too: its card lets them through, faintly.
        let editor_alpha = self.chrome.decorations.glass.and_then(|g| g.editor).unwrap_or(1.0);
        card.quad(
            Quad::filled(self.card, palette.editor_background.with_alpha(editor_alpha))
                .rounded((CARD_RADIUS * self.scale).round())
                .bordered(line, palette.card_border),
        );
        keyboard_in_overlay
    }

    /// The separators between the editor's card and the side panels / the bottom window:
    /// after all of them, their handles reaching into both sides.
    fn paint_splitters(&self, scene: &mut Scene) {
        // The left panel's separator (the project panel, or one in its place).
        if self.tools.side_on(ui_workbench::Dock::Left).is_some() {
            self.tools.splitter.paint(scene);
        }
        if self.tools.side_on(ui_workbench::Dock::Right).is_some() {
            self.tools.side_splitter.paint(scene);
        }
        if self.tool_window_shown() || self.tools.bottom_panel().is_some() {
            self.tools.terminal_splitter.paint(scene);
        }
    }

    /// The editor groups: the unfocused ones (with their state swapped in temporarily),
    /// then the current one and the dividers (a tab being dragged: over everything, last).
    fn paint_groups(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        for g in 0..self.editors.groups.len() {
            if g != self.editors.current {
                self.swap_group(g);
                self.sync_mirrors(g);
                self.sync_syntax();
                self.sync_decorations();
                self.paint_group(text, scene, false);
                self.swap_group(g);
            }
        }
        self.paint_group(text, scene, true);
        // The editor's card: what its views drew past its bottom corners cut back (the top
        // ones are the tabs' header, the app's, under the window's tint).
        let card = self.card;
        self.round_card_corners(scene, card, card.min_y() + card.size.height / 2.0);
        self.paint_group_dividers(scene);
    }

    /// The side panels, each a card with its header and content (`paint_side_panel`).
    fn paint_side_panels(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        keyboard_in_overlay: bool,
    ) {
        let mut sides = std::mem::take(&mut self.tools.sides);
        for side in &mut sides {
            self.paint_side_panel(side, keyboard_in_overlay, text, scene);
        }
        self.tools.sides = sides;
    }

    /// A side panel: a card, the header the app draws for those asking (their icon and
    /// title from the registry, their buttons), then the panel.
    fn paint_side_panel(
        &self,
        side: &mut SidePanel,
        keyboard_in_overlay: bool,
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        let b = side.bounds;
        let line = self.scale.round().max(1.0);
        // At the bottom, a card as the bottom windows' (the terminal's background).
        let bottom = side.dock == ui_workbench::Dock::Bottom;
        let background =
            if bottom { self.palette.editor_background } else { self.palette.sidebar_background };
        // Effects shown behind the panels: the card lets them through, faintly (its content
        // drawn on it, its own background left to the card).
        let glass = self.chrome.decorations.glass.map(|g| g.panels);
        let background = background.with_alpha(glass.unwrap_or(1.0));
        let palette = self.chrome.decorations.glass_palette.as_ref().unwrap_or(&self.palette);
        scene.push_layer(None).quad(
            Quad::filled(b, background)
                .rounded((CARD_RADIUS * self.scale).round())
                .bordered(line, self.palette.card_border),
        );
        if let Some(buttons) = side.panel.header() {
            let tool = self.tool(side.id);
            let title = tool.map(|t| t.title()).unwrap_or_default();
            let icon = side.panel.header_icon().or_else(|| tool.map(|t| t.icon()));
            let icon = icon.map(|i| self.icons.id_or_file(i));
            // Consecutive buttons of one group together.
            let mut actions: Vec<Vec<PanelAction>> = buttons
                .chunk_by(|a, b| a.group == b.group)
                .map(|group| {
                    group
                        .iter()
                        .map(|b| PanelAction {
                            id: b.id,
                            icon: self.icons.id_or_file(b.icon),
                            lit: false,
                        })
                        .collect()
                })
                .collect();
            // A bottom panel sizes and hides as the bottom windows do: its own group, at the
            // end (half the height, all of it, hide).
            if bottom {
                use super::tool_windows::BottomSize;
                let now = self.tools.bottom_size;
                let full = if now == BottomSize::Full { "minimize-2" } else { "maximize-2" };
                let button =
                    |id, icon, lit| PanelAction { id, icon: self.icons.id_or_file(icon), lit };
                actions.push(vec![
                    button(side_panel::BOTTOM_HALF, "view-split-down", now == BottomSize::Half),
                    button(side_panel::BOTTOM_FULL, full, false),
                    button(super::window_menu::WINDOW_MORE, "more-vertical", false),
                    button(side_panel::BOTTOM_HIDE, "minus", false),
                ]);
            } else {
                // The window's options ("⋮"), right before its own hide (or last).
                let more = PanelAction {
                    id: super::window_menu::WINDOW_MORE,
                    icon: self.icons.id_or_file("more-vertical"),
                    lit: false,
                };
                match actions.last_mut() {
                    Some(last) if last.last().is_some_and(|a| a.id == "hide") => {
                        last.insert(last.len() - 1, more);
                    }
                    _ => actions.push(vec![more]),
                }
            }
            let groups: Vec<&[PanelAction]> = actions.iter().map(Vec::as_slice).collect();
            let painter = Painter { text, scene, palette, icons: &self.icons };
            side.header.paint(icon, &title, &groups, painter);
        }
        {
            // Timed for a profile: the view's own paint only.
            let _busy = kernel::probe::time(side.id, "paint");
            side.panel.paint(ui_workbench::ViewerPaint {
                bounds: side.content,
                scale: self.scale,
                text,
                scene,
                palette,
                icons: &self.icons,
                focused: self.focus == Focus::Side(side.dock) && !keyboard_in_overlay,
                waker: self.waker.as_ref().map(|w| Arc::clone(&w.0)),
            });
        }
        self.round_card_corners(scene, b, side.content.min_y());
    }

    /// What a card's content drew past its rounded corners (a list's background filled to
    /// its square edges) cut back to the card's shape, and its border drawn again over it —
    /// for every card, whatever its content paints (the side panels, the bottom window, the
    /// editor's). From `from_y` down only (above it, the app's own header: nothing past the
    /// corners, and the window's tint behind them left as it is).
    ///
    /// The renderers clip to rectangles; the corners are cut by a ring of the window's
    /// background around the card whose inner edge is exactly the card's rounded outline
    /// (a quad `r` larger on each side, of radius `2r`, with an `r` wide border), clipped to
    /// the card: only what lies outside the outline, in the corners, is covered.
    pub(super) fn round_card_corners(&self, scene: &mut Scene, card: Rect, from_y: f32) {
        let radius = (CARD_RADIUS * self.scale).round();
        if radius <= 0.0 || card.size.width <= 0.0 || card.size.height <= 0.0 {
            return;
        }
        let top = from_y.clamp(card.min_y(), card.max_y());
        let clip = Rect::new(card.min_x(), top, card.size.width, card.max_y() - top);
        if clip.size.height <= 0.0 {
            return;
        }
        let line = self.scale.round().max(1.0);
        let ring = Rect::new(
            card.min_x() - radius,
            card.min_y() - radius,
            card.size.width + 2.0 * radius,
            card.size.height + 2.0 * radius,
        );
        let layer = scene.push_layer(Some(clip));
        layer.quad(
            Quad::filled(ring, kernel::Rgba::TRANSPARENT)
                .rounded(2.0 * radius)
                .bordered(radius, self.palette.window_background),
        );
        layer.quad(
            Quad::filled(card, kernel::Rgba::TRANSPARENT)
                .rounded(radius)
                .bordered(line, self.palette.card_border),
        );
    }

    /// The top bar: the project, the branch, the registered buttons and the window's
    /// controls; full screen, only as far as it's revealed.
    fn paint_top_bar(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let current = self.current_project();
        let menu_open = self.home.projects_menu.is_open();
        // Its buttons on the right, as registered (search, settings).
        let actions: Vec<(&'static str, icons::IconId)> = self
            .docked(ui_workbench::Dock::Top)
            .iter()
            .map(|t| (t.id(), self.icons.id_or_file(t.icon())))
            .collect();
        // What the modules say of this window's project (its shortcut).
        let badge = self
            .project_windows
            .iter()
            .find(|p| p.current)
            .and_then(|own| self.support.top_bar_badges.iter().find_map(|b| b.badge(own)));
        let shown = badge.as_ref().map(|b| b.text.clone());
        self.chrome.badge = badge;
        let model = TopBarModel {
            project: current.as_ref().map(|c| c.name.as_str()),
            branch: self.chrome.project_label.as_deref(),
            branch_button: self.branch_action().is_some(),
            badge: shown.as_deref(),
            // Full screen, the system's window buttons are gone (macOS).
            left_inset: if self.chrome.full_screen { 0.0 } else { self.chrome.titlebar_inset },
            controls: self.chrome.window_controls,
            full_screen: self.chrome.full_screen,
            maximize_hovered: self.chrome.maximize_hovered,
            actions: &actions,
            menu: self.menu_button_shown().then(|| self.menu_drawer_open()),
        };
        // Hidden (full screen): it slides down over the content as far as it's shown, and
        // isn't drawn (nor clicked: above the window) while it's gone.
        let shown =
            if self.chrome.top_bar_hides() { self.chrome.top_bar_reveal.value() } else { 1.0 };
        if self.chrome.top_bar_hides() {
            let h = TopBar::height(self.scale);
            let top = (-h * (1.0 - shown)).round();
            self.chrome
                .top_bar
                .set_bounds(Rect::new(0.0, top, self.size.width as f32, h), self.scale);
        }
        if shown > 0.0 {
            self.chrome.top_bar.paint(model, menu_open, &self.icons, text, scene, &self.palette);
        }
    }

    /// Paints the current editor group (the tabs, the breadcrumb, the editor or the viewer).
    /// `focused`: the group being typed in (the cursor and the find bar are active).
    pub(super) fn paint_group(&mut self, text: &mut TextSystem, scene: &mut Scene, focused: bool) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.seen = std::time::Instant::now();
        }
        if self.tabs.is_empty() {
            return;
        }
        self.paint_group_header(text, scene);
        self.paint_phases.mark("editor.tabs");

        // A modal / an overlay's field has the keyboard: no caret blinks behind it.
        let editor_focused = focused && self.focus == Focus::Editor && !self.keyboard_in_overlay();
        let tab = &mut self.tabs[self.active];
        // The find bar may open / close between two layouts.
        tab.sync_bounds(self.editor_area, self.scale);
        // Split: the halves scrolled together (before either is painted).
        self.sync_preview_scroll();
        self.paint_phases.mark("editor.bounds");
        let tab = &mut self.tabs[self.active];
        let editor_visible = tab.editor_visible();
        tab.sync_occurrences(&self.support);
        self.paint_phases.mark("editor.occurrences");
        // A language server's occurrences of the symbol at the cursor (nothing selected).
        if focused {
            self.sync_served_highlights();
            self.sync_bulb();
        }
        let bulb = if focused { self.bulb() } else { None };
        self.paint_phases.mark("editor.served");
        self.sync_insight();
        self.paint_phases.mark("editor.insight");
        self.sync_highlight();
        // A large document's new query: its whole search on a worker, not here.
        self.start_find_search();
        self.paint_phases.mark("editor.highlight");
        self.sync_marks();
        self.paint_phases.mark("editor.marks");
        self.sync_toolbar_editor();
        self.paint_phases.mark("editor.toolbar_sync");
        // What the modules mark beside its lines (the changed lines, the blame).
        let decorated = self.decorations_of(self.tabs[self.active].doc.id());
        self.paint_phases.mark("editor.decorated");
        let pointer = self.pointer_offset();
        let palette = &self.palette;
        let tab = &mut self.tabs[self.active];
        // What the modules hide (a `.env` file's secrets), but under the pointer or a caret.
        tab.sync_concealed(&self.support, pointer);
        self.paint_phases.mark("editor.concealed");
        // The colors on the visible lines (recomputed only on new text / scrolling).
        let visible = tab.view.visible_bytes(&tab.doc, &tab.folds);
        let key = (tab.doc.id(), tab.doc.version(), visible.clone());
        let support = &self.support;
        let swatches = self.swatches.get_or_update(key, || find_swatches(support, tab, &visible));
        self.paint_phases.mark("editor.swatches");
        // Different fields of the tab: the matches are borrowed without copying.
        let current = tab.find.current_index();
        let matches: &[std::ops::Range<usize>] =
            if tab.find_open { tab.find.matches(&tab.doc) } else { &[] };
        let current = current.and_then(|i| matches.get(i));
        // A module's bar's marks (a console's statements) and the lines it highlights.
        let highlight = tab.toolbar.as_ref().and_then(|t| t.highlight_lines(&tab.doc));
        let current_statement = tab.toolbar.as_ref().and_then(|t| t.current_lines(&tab.doc));
        let focus = tab.toolbar.as_ref().and_then(|t| t.focus_of(&tab.doc, palette));
        let statements: &[ui_workbench::StatementLines] = match tab.toolbar.as_mut() {
            Some(toolbar) => toolbar.statement_lines(&tab.doc),
            None => &[],
        };
        let link = self
            .link
            .as_ref()
            .filter(|l| focused && l.tab == self.active && l.label.is_some())
            .map(|l| &l.range);
        let decorations = Decorations {
            occurrences: &tab.occurrences,
            link,
            errors: &tab.diagnostics.error_ranges,
            deprecated: &tab.diagnostics.struck_ranges,
            warnings: &tab.diagnostics.warning_ranges,
            unused: &tab.diagnostics.unused_ranges,
            colors: &self.colors,
            matches,
            current_match: current,
            folds: &tab.folds,
            swatches: &swatches.lines,
            vcs: decorated.as_ref().map_or(&[], |d| d.marks.as_slice()),
            marks_generation: tab.marks_generation,
            annotations: decorated
                .as_ref()
                .and_then(|d| d.annotations.as_deref())
                .map_or(&[], Vec::as_slice),
            statements,
            highlight,
            current: current_statement,
            focus,
            bulb,
            lenses: &tab.insight.lines,
            concealed: &tab.concealed.hidden,
        };
        tab.view.set_scrollbar_style(ScrollbarStyle::from_palette(palette));
        if editor_visible {
            // Split with the view having the keys: the text's caret rests.
            let caret = editor_focused && tab.viewer.as_ref().is_none_or(|v| !v.has_keys());
            tab.view.paint(&tab.doc, decorations, caret, text, scene, palette);
            self.paint_phases.mark("editor.text");
            self.paint_error_preview(text, scene);
            if focused {
                self.paint_editor_popups(text, scene);
            }
        }
        self.paint_viewer_and_find(text, scene, editor_focused);
        self.paint_phases.mark("editor.viewer");
        self.saw_preview_scroll();
        self.paint_toolbar(text, scene);
        self.paint_phases.mark("editor.toolbar");
    }

    /// The group's tab bar and its breadcrumb.
    fn paint_group_header(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let palette = &self.palette;
        let titles: Vec<String> = self.tabs.iter().map(Tab::title).collect();
        // A bar's mark after the title (a console's open transaction).
        let badges: Vec<Option<(String, Rgba)>> = self
            .tabs
            .iter()
            .map(|t| t.toolbar.as_ref().and_then(|b| b.bar.tab_badge(palette)))
            .collect();
        let names: Vec<String> = self.tabs.iter().map(Tab::icon_name).collect();
        let (support, icons, theme) = (&self.support, &self.icons, self.theme_generation);
        let file_icons = &mut self.file_icons;
        let root = self.project.as_ref().map(|p| p.root_path());
        let tabs: Vec<TabTitle<'_>> = self
            .tabs
            .iter()
            .zip(titles.iter().zip(&names).zip(&badges))
            .map(|(t, ((title, name), badge))| TabTitle {
                title,
                dirty: t.is_dirty(),
                pinned: t.pinned,
                icon: Some(tab_icon(
                    file_icons.get(support, icons, palette, theme, name),
                    t,
                    icons,
                    palette,
                )),
                accent: t
                    .viewer
                    .as_ref()
                    .and_then(|v| v.viewer.accent(palette))
                    .or_else(|| t.toolbar.as_ref().and_then(|b| b.bar.accent(palette))),
                badge: badge.as_ref().map(|(b, c)| (b.as_str(), *c)),
                locked: t.locked(),
                outside: t.doc.path().is_some_and(|path| outside_project(path, root)),
            })
            .collect();
        let navigation = self.navigation();
        let painter = Painter { text, scene, palette, icons: &self.icons };
        self.tab_bar.paint(&tabs, self.active, navigation, painter);
        self.crumbs();
        let crumbs = self.crumbs.value().map_or(&[][..], |(c, _)| c.as_slice());
        self.breadcrumbs.paint(crumbs, &self.icons, text, scene, &self.palette);
    }

    /// Syntax colors: computed for the visible area plus one more screen height up and
    /// down, reused until the text, the parse or the theme changes, or until the visible
    /// area leaves the computed one.
    fn sync_highlight(&mut self) {
        let tab = &mut self.tabs[self.active];
        self.colors.clear();
        let Some(snapshot) = tab.syntax.snapshot() else { return };
        let visible = tab.view.visible_bytes(&tab.doc, &tab.folds);
        // Rows too long to highlight whole (a minified file): only their drawn parts.
        let windows = tab.view.long_line_windows(&tab.doc, &tab.folds);
        let (version, parses, theme) =
            (tab.doc.version(), tab.syntax.parses(), self.theme_generation);
        let valid = |k: &HighlightKey| {
            (k.version, k.parses, k.theme) == (version, parses, theme)
                && match &windows {
                    Some(windows) => k.windows.as_ref() == Some(windows),
                    None => {
                        k.windows.is_none()
                            && k.range.start <= visible.start
                            && visible.end <= k.range.end
                    }
                }
        };
        if tab.highlight.get_if(valid).is_none() {
            let len = tab.doc.buffer().len();
            let text_snapshot = tab.doc.buffer().snapshot();
            let range = match &windows {
                Some(windows) => {
                    self.highlighter.highlight_ranges(
                        snapshot,
                        &text_snapshot,
                        windows,
                        &mut self.spans,
                    );
                    visible.clone()
                }
                None => {
                    let margin = visible.end - visible.start;
                    let range =
                        visible.start.saturating_sub(margin)..(visible.end + margin).min(len);
                    self.highlighter.highlight(
                        snapshot,
                        &text_snapshot,
                        range.clone(),
                        &mut self.spans,
                    );
                    range
                }
            };
            let mut spans = Vec::with_capacity(self.spans.len());
            for span in &self.spans {
                let color = *self
                    .scope_colors
                    .entry(span.scope)
                    .or_insert_with(|| scope_color(&self.support, self.theme.as_ref(), span.scope));
                if let Some(style) = color {
                    spans.push(style.span(span.range.clone()));
                }
            }
            tab.highlight.set(HighlightKey { version, parses, theme, range, windows }, spans);
        }
        // Only what touches the visible area, in the same order (the order decides which color
        // wins on overlap).
        self.colors.extend(
            tab.highlight
                .value()
                .into_iter()
                .flatten()
                .filter(|s| s.range.start < visible.end && s.range.end > visible.start)
                .cloned(),
        );
    }

    /// Syntax errors, recomputed only on a new snapshot.
    /// Plus the features newer than the chosen language level ("requires PHP 8.0") and
    /// the requirements the project's manifest misses (looked up only then).
    fn sync_marks(&mut self) {
        let tab = &self.tabs[self.active];
        // Its bar asks for the grammar's doubts too (a console's setting).
        let hints = tab.toolbar.as_ref().is_some_and(|t| t.bar.grammar_hints());
        // Where its language's server alone answers, it reports the syntax errors itself:
        // the built-in engine's aren't shown twice (unless it reports none: HTML's). The
        // built-in engine's analyses (deprecated, unused, newer than the level, missing from
        // the manifest) stay, except where a server's diagnostic marks the same place.
        let (engine, kept) = self.active_engine();
        let alone = self.server_alone();
        let served = engine.uses_server();
        let errors_key = (
            tab.syntax.parses(),
            tab.diagnostics.issues_generation + (self.data.generations.files() << 32),
            self.data.diagnostics.generation() * 16
                + u64::from(hints)
                + 2 * u64::from(alone)
                + 4 * u64::from(kept.deprecated)
                + 8 * u64::from(served),
        );
        if tab.syntax.snapshot().is_none() || tab.diagnostics.errors_version == Some(errors_key) {
            return;
        }
        let missing = self.missing_requirements();
        // The diagnostics the project's modules found in this file (its checkers').
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let tab = &self.tabs[self.active];
        let relative =
            tab.doc.path().zip(root.as_deref()).and_then(|(p, r)| p.strip_prefix(r).ok());
        let language = tab.syntax.language().clone();
        let intelligence = self.support.intelligence.iter().find(|i| i.language() == language);
        let word_chars = intelligence.map_or("", |i| i.word_chars());
        let checked: Vec<Mark> = relative
            .map(|file| {
                self.data
                    .diagnostics
                    .of(file)
                    .filter_map(|p| diagnostics::mark_of(p, tab.doc.buffer(), word_chars))
                    .collect()
            })
            .unwrap_or_default();
        // Where the servers' own diagnostics are (the built-in engine's there aren't shown).
        let by_server = match relative.filter(|_| served) {
            Some(file) => diagnostics::Covered::new(
                self.data
                    .diagnostics
                    .of_sources(file, code_intelligence::servers::is_server_source)
                    .filter_map(|p| diagnostics::mark_of(p, tab.doc.buffer(), word_chars))
                    .map(|m| m.range),
            ),
            None => diagnostics::Covered::default(),
        };
        let own = |range: &std::ops::Range<usize>| !by_server.meets(range);
        let tab = &mut self.tabs[self.active];
        let Some(snapshot) = tab.syntax.snapshot() else { return };
        tab.diagnostics.errors = if alone { Vec::new() } else { snapshot.errors() };
        let quiet = if hints { snapshot.quiet_errors() } else { Vec::new() };
        let mark = |range: &std::ops::Range<usize>, (message, code), warning| Mark {
            range: range.clone(),
            message,
            code,
            warning,
            weak: false,
            struck: false,
        };
        let marks: Vec<Mark> = tab
            .diagnostics
            .errors
            .iter()
            .filter(|e| own(&e.range))
            .map(|e| mark(&e.range, error_message(&tab.doc, e), false))
            .chain(
                tab.diagnostics
                    .level_issues
                    .iter()
                    .filter(|i| own(&i.range))
                    .map(|i| mark(&i.range, level_message(i), i.is_warning())),
            )
            .chain(
                tab.diagnostics
                    .deprecated
                    .iter()
                    .filter(|d| kept.deprecated || own(&d.range))
                    .map(|d| Mark { struck: true, ..mark(&d.range, deprecated_message(d), true) }),
            )
            .chain(missing.iter().filter(|(r, _)| own(&r.range)).map(|(r, manifest)| {
                let message = tr!("requirement.missing", manifest = *manifest);
                mark(&r.range, (message, Some(r.name.clone())), true)
            }))
            .chain(
                tab.diagnostics
                    .problems
                    .iter()
                    .filter(|p| own(&p.range))
                    .map(|p| mark(&p.range, problem_message(p), true)),
            )
            .chain(
                tab.diagnostics
                    .unused
                    .iter()
                    .filter(|u| own(&u.range))
                    .map(|u| Mark { weak: true, ..mark(&u.range, unused_message(u), true) }),
            )
            .chain(checked)
            .chain(quiet.iter().map(|e| {
                let (message, code) = error_message(&tab.doc, e);
                let message = format!("{} · {message}", i18n::t("error.doubt"));
                Mark { weak: true, ..mark(&e.range, (message, code), true) }
            }))
            .collect();
        tab.diagnostics.set_marks(marks, errors_key);
        tab.marks_generation += 1;
    }

    /// The preview of an error's area (mouse on its marker on the scroll strip).
    fn paint_error_preview(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let tab = &mut self.tabs[self.active];
        let Some(offset) = tab.view.hovered_error() else { return };
        let Some(i) = tab.diagnostics.marks.iter().position(|m| m.range.start == offset) else {
            return;
        };
        let error = tab.diagnostics.marks[i].range.clone();
        let buffer = tab.doc.buffer();
        let line = buffer.line_of(error.start.min(buffer.len()));
        let from = buffer.line_start(line.saturating_sub(2));
        let to = buffer.line_end((line + 2).min(buffer.line_count().saturating_sub(1)));
        let mut colors = Vec::new();
        if let Some(snapshot) = tab.syntax.snapshot() {
            let text_snapshot = tab.doc.buffer().snapshot();
            self.highlighter.highlight(snapshot, &text_snapshot, from..to, &mut self.spans);
            for span in &self.spans {
                let color = *self
                    .scope_colors
                    .entry(span.scope)
                    .or_insert_with(|| scope_color(&self.support, self.theme.as_ref(), span.scope));
                if let Some(style) = color {
                    colors.push(style.span(span.range.clone()));
                }
            }
        }
        let (message, code) = tab
            .diagnostics
            .marks
            .get(i)
            .map(|m| (m.message.clone(), m.code.clone()))
            .unwrap_or_default();
        let title = tr!("error.line", line = line + 1);
        let spec = ui_workbench::ErrorPreviewSpec {
            error,
            message: &message,
            code: code.as_deref(),
            line_label: &title,
            colors: &colors,
        };
        let painter = Painter { text, scene, palette: &self.palette, icons: &self.icons };
        tab.view.paint_error_preview(&tab.doc, spec, painter);
    }

    /// Over the focused group's visible editor: the tooltip of the error under the mouse,
    /// the parameter hint, the code hover and the link's badge.
    fn paint_editor_popups(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let palette = &self.palette;
        let tab = &mut self.tabs[self.active];
        // The tooltip of the error under the mouse (on its underline).
        if let Some((i, at)) = self.error_tip
            && let Some(mark) = tab.diagnostics.marks.get(i)
        {
            let tip = ui_workbench::ErrorTip {
                message: &mark.message,
                code: mark.code.as_deref(),
                warning: mark.warning,
                weak: mark.weak,
            };
            tab.view.paint_error_tooltip(at, tip, text, scene, palette);
        }
        if self.param_hint.is_open() {
            let caret = tab.view.caret_rect(&tab.doc, &tab.folds, text, palette);
            let bounds = tab.view.bounds();
            self.param_hint.paint(caret, bounds, self.scale, text, scene, palette);
        }
        self.code_hover.paint(tab.view.bounds(), self.scale, text, scene, palette);
        if let Some(label) = self
            .link
            .as_ref()
            .filter(|l| l.tab == self.active)
            .filter(|_| !self.code_hover.is_shown())
            .and_then(|l| l.label.as_deref())
        {
            let key =
                i18n::t(if cfg!(target_os = "macos") { "link.click" } else { "link.click_other" });
            tab.view.paint_link_badge(&key, label, text, scene, palette);
        }
    }

    /// The tab's viewer (when it has one) and its find bar.
    fn paint_viewer_and_find(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        editor_focused: bool,
    ) {
        let palette = &self.palette;
        let tab = &mut self.tabs[self.active];
        if let Some(v) = &mut tab.viewer {
            paint_viewer(
                v,
                &tab.doc,
                self.editor_area,
                self.scale,
                Painter { text, scene, palette, icons: &self.icons },
                self.waker.as_ref().map(|w| Arc::clone(&w.0)),
            );
        }
        if tab.find_open {
            let counter = tab.find.counter(&tab.doc);
            let error = tab.find.error().is_some();
            let query = tab.find.query.clone();
            let find_area = tab.find_area(self.editor_area, self.scale);
            let spec = ui_workbench::FindBarSpec {
                bounds: find_area,
                options: &query,
                counter,
                error,
                focused: !editor_focused,
                scale: self.scale,
            };
            tab.find_bar.paint(spec, Painter { text, scene, palette, icons: &self.icons });
        }
    }

    // ─── The empty screen ───

    /// The empty screen: the projects (history) and the actions, with a filter and keys.
    pub(super) fn paint_empty_state(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        // Read again only when the list changed (not on every frame: on a first launch it
        // stays empty).
        let generation = crate::recent::generation();
        if self.home.recent_generation != Some(generation) {
            self.home.recent = self.recent_projects();
            self.home.recent_generation = Some(generation);
        }
        self.sync_home_sessions();
        let current = self.current_project();
        let query = self.home.picker.query.text().trim().to_owned();
        let key = self.project.as_ref().map(|_| {
            let files = self.data.indexer.as_ref().and_then(|i| i.peek_files());
            let index = files.map_or(0, |f| Arc::as_ptr(&f) as usize);
            (
                query.clone(),
                self.data.id,
                self.data.generations.files(),
                self.data.recent_files.len(),
                self.data.recent_files.last().cloned(),
                index,
                (self.theme_generation, self.icons.file_set(), self.icons.look()),
            )
        });
        if self.home.files.as_ref().map(|(made, _)| made) != key.as_ref() {
            let files = self.make_home_files(&query);
            self.home.files = key.map(|key| (key, files));
        }
        let files = self.home.files.as_ref().and_then(|(_, files)| files.as_ref());
        if files.is_some_and(|(_, searching)| *searching) {
            self.ensure_index();
        }
        let files = self.home.files.as_ref().and_then(|(_, files)| files.as_ref());
        let name = current.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        let content = match files {
            Some((files, searching)) => {
                ui_workbench::HomeContent::Project { name: &name, files, searching: *searching }
            }
            None => ui_workbench::HomeContent::Projects,
        };
        self.home.picker.focused = self.focus == Focus::Editor;
        self.home.picker.paint_home(
            self.editor_area,
            self.scale,
            content,
            current.as_ref(),
            &self.home.recent,
            Painter { text, scene, palette: &self.palette, icons: &self.icons },
        );
    }

    /// The empty screen's files: with a project open, its recent files or, with a filter,
    /// the ones found (`true`).
    fn make_home_files(&mut self, query: &str) -> Option<HomeFiles> {
        // The search (folders, then files) runs before the rows are built.
        let found = if self.project.is_some() && !query.is_empty() {
            self.search_paths(query, 4, 10)
        } else {
            PathResults::default()
        };
        let root = self.project.as_ref()?.root_path().to_path_buf();
        let theme = self.theme_generation;
        let (support, icons, palette) = (&self.support, &self.icons, &self.palette);
        let disk = &*self.ports.disk;
        let file_icons = &mut self.file_icons;
        let mut entry = |path: PathBuf| {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let detail = path
                .parent()
                .and_then(|d| d.strip_prefix(&root).ok())
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            let icon = Some(file_icons.get(support, icons, palette, theme, &name));
            ui_workbench::HomeFile { name, detail, path, icon }
        };
        if query.is_empty() {
            let files: Vec<_> = self
                .data
                .recent_files
                .iter()
                .rev()
                .filter(|p| disk.kind(p).is_some())
                .take(8)
                .cloned()
                .map(&mut entry)
                .collect();
            return Some((files, false));
        }
        // The same search as everywhere: the folders first, then the files.
        let folder_icon = icons.id("folder").map(|id| (id, palette.accent));
        let mut rows = Vec::new();
        for m in found.folders {
            let mut row = entry(root.join(&m.path));
            row.icon = folder_icon;
            rows.push(row);
        }
        rows.extend(found.files.into_iter().map(|m| entry(root.join(&m.path))));
        Some((rows, true))
    }
}

/// The colors written on the tab's visible lines (`visible`): the first literal of each
/// line (lines over 2000 bytes are skipped), in the language at its first character.
fn find_swatches(
    support: &LanguageSupport,
    tab: &Tab,
    visible: &std::ops::Range<usize>,
) -> Swatches {
    let mut found = Swatches::default();
    let Some(provider) = support.colors.first() else { return found };
    let buffer = tab.doc.buffer();
    let first = buffer.line_of(visible.start.min(buffer.len()));
    let last = buffer.line_of(visible.end.min(buffer.len()));
    for line in first..=last {
        let start = buffer.line_start(line);
        let source = buffer.slice(start..buffer.line_end(line));
        if source.len() > 2000 {
            continue;
        }
        let indent = source.len() - source.trim_start().len();
        let language = tab.syntax.snapshot().map_or_else(
            || tab.syntax.language().clone(),
            |s| s.language_at(start + indent).clone(),
        );
        if let Some(mut literal) = provider.find(language.as_str(), &source).into_iter().next() {
            found.lines.push((line as u32, literal.color));
            literal.range = start + literal.range.start..start + literal.range.end;
            found.literals.push(literal);
        }
    }
    found
}

/// "Unused element", with the name as the code fragment.
fn unused_message(u: &code_intelligence::Unused) -> (String, Option<String>) {
    (i18n::t(&format!("unused.{}", u.kind)), Some(u.name.clone()))
}

/// "Unknown class", "Too few arguments: 2 required, 1 given", with what it's about as the
/// code fragment.
fn problem_message(p: &code_intelligence::Problem) -> (String, Option<String>) {
    let key = format!("problem.{}", p.kind);
    let message = match p.arguments {
        Some((expected, given)) => tr!(&key, expected = expected as i64, given = given as i64),
        None => i18n::t(&key),
    };
    (message, Some(p.name.clone()))
}

/// "Deprecated element", with the name as the code fragment.
fn deprecated_message(d: &code_intelligence::Deprecated) -> (String, Option<String>) {
    (i18n::t(&format!("deprecated.{}", d.kind)), Some(d.name.clone()))
}

/// "Match expressions require PHP 8.0", with the construct as code (`match`, `??`,
/// `str_contains()`) when it has one.
fn level_message(issue: &code_intelligence::LevelIssue) -> (String, Option<String>) {
    let code = issue.code.clone();
    // Removed or deprecated by the level: its own message (`level.removed_class`…).
    if issue.feature.starts_with("removed_") || issue.is_warning() {
        let name = issue.name.clone().unwrap_or_default();
        let key = format!("level.{}", issue.feature);
        return (i18n::tr!(&key, name = name, version = issue.since), code);
    }
    let feature = i18n::t(&format!("level.feature.{}", issue.feature));
    (i18n::tr!("level.requires", feature = feature, version = issue.since), code)
}

/// Tab `tab`'s icon: the one its view asks for (an agent's session: the agent's), else its
/// file's (`file`), or the one its bar names (a console's driver's: a server's) in the muted
/// text's color.
fn tab_icon(file: (IconId, Rgba), tab: &Tab, icons: &Icons, palette: &Palette) -> (IconId, Rgba) {
    if let Some(id) =
        tab.viewer.as_ref().and_then(|v| v.viewer.tab_icon()).and_then(|i| icons.id(i))
    {
        return (id, palette.text_muted);
    }
    tab.toolbar
        .as_ref()
        .and_then(|t| t.bar.icon())
        .and_then(|name| icons.id(name))
        .map_or(file, |id| (id, palette.text_muted))
}

/// A file that isn't the open project's (dropped in, opened from elsewhere); with no project
/// open, or a tab that isn't a file, nothing is.
pub(super) fn outside_project(path: &std::path::Path, root: Option<&std::path::Path>) -> bool {
    root.is_some_and(|root| !path.starts_with(root)) && !super::disk::is_virtual(path)
}
