//! The tool windows as a project keeps them.

use super::*;

impl Workbench {
    /// The bottom windows' views to reopen at the next start (those that ask: the
    /// terminal's shells), with their titles, pins and groups; split, the groups' layout.
    pub(in crate::workbench) fn saved_tool_windows(&self) -> Vec<workbench_model::SavedToolWindow> {
        self.tools.windows.iter().filter_map(saved_tool_window).collect()
    }

    /// Makes the saved views again (each told its project) in their groups, and shows the
    /// window that was shown (if it has views).
    pub(in crate::workbench) fn restore_tool_windows(
        &mut self,
        saved: &[workbench_model::SavedToolWindow],
        bottom: Option<&str>,
    ) {
        let id = self.project.as_ref().map(|_| self.data.id);
        let queries = self.project.is_some().then(|| self.queries()).flatten();
        let restored =
            ui_workbench::Restored { project: self.project_root(), id, memento: None, queries };
        for window in saved {
            let count = window.views.iter().map(|v| v.group + 1).max().unwrap_or(1);
            // A split whose tree doesn't match its groups: all in one.
            let tree = (count > 1)
                .then(|| SplitTree::from_spec(&window.split, count))
                .flatten()
                .unwrap_or_default();
            let count = if tree == SplitTree::default() { 1 } else { count };
            let mut made = ToolWindow::new(&window.id);
            made.tree = tree;
            made.groups = (0..count).map(|_| ToolGroup::default()).collect();
            for view in &window.views {
                let path = PathBuf::from(format!("{}.{}", view.title, view.extension));
                let provider = self
                    .support
                    .viewers
                    .iter()
                    .find(|p| p.kind(&path) == Some(ui_workbench::ViewerKind::Replace));
                let Some((Ok(mut viewer), provider)) = provider.map(|p| (p.open(&path), p.id()))
                else {
                    continue;
                };
                self.give_sizes(viewer.as_mut());
                viewer.set_data(&ui_workbench::Restored {
                    memento: view.memento.clone(),
                    ..restored.clone()
                });
                made.groups[view.group.min(count - 1)].views.push(PanelView {
                    id: ViewId::next(),
                    // Its own name first (a session saved in another language, or before
                    // views had one), then the one saved.
                    key: viewer
                        .panel_key()
                        .map(str::to_owned)
                        .or_else(|| view.key.clone())
                        .unwrap_or_else(|| view.title.clone()),
                    title: view.title.clone(),
                    extension: view.extension.clone(),
                    pinned: view.pinned,
                    provider,
                    viewer,
                    text_told: 0,
                    shown: false,
                    active: false,
                });
            }
            if made.is_empty() {
                continue;
            }
            made.current = window.group.min(count - 1);
            for (g, group) in made.groups.iter_mut().enumerate() {
                let active = if g == made.current {
                    window.active
                } else {
                    window.actives.get(g).copied().unwrap_or(0)
                };
                group.active = active.min(group.views.len().saturating_sub(1));
            }
            made.prune();
            made.hidden_since = Some(std::time::Instant::now());
            self.tools.windows.retain(|w| w.id != window.id);
            self.tools.windows.push(made);
        }
        if let Some(id) = bottom.filter(|id| self.tools.windows.iter().any(|w| w.id == *id)) {
            self.set_bottom(Some(id));
        } else if let Some(id) = bottom.filter(|id| {
            self.tool(id).is_some_and(|t| {
                t.dock() == ui_workbench::Dock::Bottom
                    && t.content() == ui_workbench::ToolContent::Panel
            })
        }) {
            // A panel docked at the bottom (Services), shown as it was.
            self.show_side_panel(id);
        }
    }
}
