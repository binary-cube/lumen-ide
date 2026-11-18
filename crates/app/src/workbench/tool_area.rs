//! The tool windows' places around the card: the side strips, the side panels and their
//! separators, the bottom area and its separator.

use super::*;

/// The tool strips, the side panels and the bottom tool windows, with their sizes.
#[derive(Debug)]
pub(super) struct ToolArea {
    /// The left toolbar (the tool windows' buttons).
    pub strip: ToolStrip,
    /// The right toolbar (the module panels) and the side panel separator.
    pub right_strip: ToolStrip,
    /// The separator between the left panel and the card.
    pub splitter: Splitter,
    pub side_splitter: Splitter,
    /// The left panel's card (the project panel's place); empty if none is open.
    pub sidebar: Rect,
    /// The separator above the bottom area. The docks' sizes (one per dock, whatever tool
    /// window it shows) are the project's (`ProjectData::sizes`).
    pub terminal_splitter: Splitter,
    /// The side panels open (one per side at most).
    pub sides: Vec<SidePanel>,
    /// Side panels hidden, kept as they were until shown again.
    pub parked: Vec<SidePanel>,
    /// The modules' tool windows at the bottom; the window the bottom area shows (one at a
    /// time, any: the terminal's or a module's, by id); its area, its header's buttons and
    /// hover, a drag in its view.
    pub windows: Vec<tool_windows::ToolWindow>,
    pub bottom: Option<String>,
    pub bottom_bounds: Rect,
    /// Its own size, half the height or all of it.
    pub bottom_size: tool_windows::BottomSize,
    pub hits: Vec<(Rect, tool_windows::ToolHit)>,
    pub hover: Option<tool_windows::ToolHit>,
    pub panel_dragging: bool,
    /// A press on one of the shown window's tabs, maybe dragging it to another place.
    pub tab_drag: Option<tool_windows::ToolTabDrag>,
    /// The tool strip menu shown (its button stays lit).
    pub menu_open: Option<&'static str>,
    /// The side panel whose own menu is shown (its rows come back to it).
    pub menu_panel: Option<&'static str>,
    /// The side panel the mouse was last over (told when it leaves).
    pub hovered: Option<&'static str>,
    /// What each panel kept of itself (the project's session), given when it's made.
    pub mementos: std::collections::HashMap<String, String>,
}

impl ToolArea {
    pub fn new(palette: &Palette) -> Self {
        let splitter_style = SplitterStyle::from_palette(palette);
        let strip_style = ToolStripStyle::from_palette(palette);
        Self {
            strip: ToolStrip::new(strip_style.clone()),
            right_strip: ToolStrip::new(strip_style),
            splitter: Splitter::new(splitter_style.clone(), Orientation::Vertical),
            side_splitter: Splitter::new(splitter_style.clone(), Orientation::Vertical),
            sidebar: Rect::default(),
            terminal_splitter: Splitter::new(splitter_style, Orientation::Horizontal),
            sides: Vec::new(),
            parked: Vec::new(),
            windows: Vec::new(),
            bottom: None,
            bottom_bounds: Rect::default(),
            bottom_size: tool_windows::BottomSize::Kept,
            hits: Vec::new(),
            hover: None,
            panel_dragging: false,
            tab_drag: None,
            menu_open: None,
            menu_panel: None,
            hovered: None,
            mementos: std::collections::HashMap::new(),
        }
    }

    /// The strips and separators in `palette`'s colors.
    pub fn set_palette(&mut self, palette: &Palette) {
        self.splitter.set_style(SplitterStyle::from_palette(palette));
        self.terminal_splitter.set_style(SplitterStyle::from_palette(palette));
        self.strip.set_style(ToolStripStyle::from_palette(palette));
        self.right_strip.set_style(ToolStripStyle::from_palette(palette));
        self.side_splitter.set_style(SplitterStyle::from_palette(palette));
    }

    /// The side panel open on `dock`.
    pub fn side_on(&self, dock: ui_workbench::Dock) -> Option<&SidePanel> {
        self.sides.iter().find(|s| s.dock == dock)
    }

    /// The panel shown at the bottom (Services), if one is.
    pub fn bottom_panel(&self) -> Option<&SidePanel> {
        self.side_on(ui_workbench::Dock::Bottom)
    }

    /// What every panel made keeps of itself, by its id (the shown and the hidden ones; the
    /// ones not made since keep what they kept).
    pub fn panel_mementos(&self) -> Vec<(String, String)> {
        let mut out = self.mementos.clone();
        for side in self.sides.iter().chain(&self.parked) {
            if let Some(memento) = side.panel.memento() {
                out.insert(side.id.to_owned(), memento);
            }
        }
        let mut out: Vec<(String, String)> = out.into_iter().collect();
        out.sort();
        out
    }

    /// The side panel under `p`.
    pub fn side_at(&mut self, p: Point) -> Option<&mut SidePanel> {
        self.sides.iter_mut().find(|s| s.bounds.contains(p))
    }

    /// The module window shown at the bottom.
    pub fn window(&self) -> Option<&tool_windows::ToolWindow> {
        let id = self.bottom.as_deref()?;
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self) -> Option<&mut tool_windows::ToolWindow> {
        let id = self.bottom.clone()?;
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The view shown in the bottom window's current group.
    pub fn panel_viewer(&mut self) -> Option<&mut Box<dyn ui_workbench::Viewer>> {
        let group = self.window_mut()?.group_mut();
        let active = group.active;
        group.views.get_mut(active).map(tool_windows::PanelView::viewer_mut)
    }

    /// Renames view `i` of the shown window's current group (empty: kept as it was).
    pub fn rename_view(&mut self, i: usize, name: &str) {
        let name = name.trim();
        if let Some(v) = self.window_mut().and_then(|w| w.group_mut().views.get_mut(i))
            && !name.is_empty()
        {
            name.clone_into(&mut v.title);
        }
    }

    /// Pins / unpins view `i` of the current group: pinned ones move first (in their
    /// order), the active one stays.
    pub fn toggle_pin_view(&mut self, i: usize) {
        let Some(w) = self.window_mut() else { return };
        let group = w.group_mut();
        let Some(view) = group.views.get_mut(i) else { return };
        view.pinned = !view.pinned;
        let active = group.views.get(group.active).map(|v| v.id);
        group.views.sort_by_key(|v| !v.pinned);
        if let Some(i) = active.and_then(|id| group.views.iter().position(|v| v.id == id)) {
            group.active = i;
        }
    }

    /// Closes the current group's views but `i` (and the pinned and fixed ones).
    pub fn close_other_views(&mut self, i: usize) {
        let Some(w) = self.window_mut() else { return };
        let group = w.group_mut();
        let Some(keep) = group.views.get(i).map(|v| v.id) else { return };
        group.views.retain(|v| v.id == keep || v.pinned || !v.closable());
        group.active = group.views.iter().position(|v| v.id == keep).unwrap_or(0);
    }
}
