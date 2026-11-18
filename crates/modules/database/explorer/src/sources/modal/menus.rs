//! The list's right-click menus and what their rows do: a source's color, moving it into a folder
//! or another scope.

use super::*;

impl SourcesView {
    /// Folder `id` selected (its name and color on the right).
    pub fn select_folder(&mut self, id: FolderId) {
        let Some(editor) = &mut self.editor else { return };
        editor.select_folder(id);
        self.focus = None;
        self.close_popup();
        self.form_scroll = 0.0;
        self.error = None;
    }

    /// `item` moved to `place` (a source to another list's folder goes to that list).
    pub fn move_to(&mut self, item: Item, place: Place) -> bool {
        let Some(editor) = &mut self.editor else { return false };
        let moved = match item {
            Item::Source(id) => editor.move_source(id, place),
            Item::Folder(id) => editor.move_folder(id, place),
        };
        if moved {
            self.confirm_close = false;
            self.error = None;
        }
        moved
    }

    /// Where a drop at `p` puts what's dragged: a folder's row into it, a source's row
    /// beside it (its folder), a list's title or what's empty of it at its top.
    pub(super) fn place_at(&self, p: Point) -> Option<Place> {
        let editor = self.editor.as_ref()?;
        if !self.list_area.contains(p) {
            return None;
        }
        Some(match self.hit(p) {
            Some(Hit::Folder(id, _)) => Place::Folder(id),
            Some(Hit::Section(scope)) => Place::Top(scope),
            Some(Hit::Row(i)) => {
                let source = &editor.entries.get(i)?.source;
                match source.folder.and_then(|f| editor.folder(f)) {
                    Some(f) if f.scope == source.scope => Place::Folder(f.id),
                    _ => Place::Top(source.scope),
                }
            }
            _ => Place::Top(self.last_scope_above(p)),
        })
    }

    /// The list whose rows end above `p` (an empty space below the global list's is its).
    pub(super) fn last_scope_above(&self, p: Point) -> Scope {
        self.hits
            .iter()
            .filter_map(|(r, h)| match h {
                Hit::Section(scope) if r.min_y() <= p.y => Some((r.min_y(), *scope)),
                _ => None,
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(Scope::Project, |(_, s)| s)
    }

    /// A place's name in the "Move To" menu: a list's top, or a folder's path.
    pub(super) fn place_label(&self, place: Place) -> String {
        match place {
            Place::Top(Scope::Project) => t("datasources.place_project"),
            Place::Top(Scope::Global) => t("datasources.place_global"),
            Place::Folder(id) => {
                let Some(editor) = &self.editor else { return String::new() };
                database::folders::folder_path(&editor.folders, id)
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            }
        }
    }

    /// The colors offered (the menu's and the form's): none (`inherit`'s label), then the
    /// named ones.
    pub(super) fn color_menu(
        &self,
        prefix: &str,
        inherit: &str,
        now: Option<SourceColor>,
    ) -> ViewMenuEntry {
        let mut entries = vec![ViewMenuEntry::item(format!("{prefix}none"), t(inherit))];
        for color in SourceColor::NAMED {
            let mut label = t(&format!("datasources.color_{}", color.key()));
            if now == Some(color) {
                label = format!("{label} ✓");
            }
            entries.push(ViewMenuEntry::item(format!("{prefix}{}", color.key()), label));
        }
        ViewMenuEntry::Submenu {
            label: t("datasources.menu_color"),
            icon: Some("palette"),
            entries,
        }
    }

    /// The "Move To" submenu of `item`.
    pub(super) fn move_menu(&self, item: Item, prefix: &str) -> Option<ViewMenuEntry> {
        let editor = self.editor.as_ref()?;
        let entries: Vec<ViewMenuEntry> = editor
            .places_for(item)
            .into_iter()
            .map(|place| {
                let id = match place {
                    Place::Top(Scope::Project) => "project".to_owned(),
                    Place::Top(Scope::Global) => "global".to_owned(),
                    Place::Folder(f) => f.to_string(),
                };
                ViewMenuEntry::item(format!("{prefix}{id}"), self.place_label(place))
            })
            .collect();
        Some(ViewMenuEntry::Submenu { label: t("datasources.menu_move_to"), icon: None, entries })
    }

    /// The right-click menu of the list at `p`: a folder's, a source's, or a new folder.
    pub(super) fn list_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if !self.list_area.contains(p) {
            return None;
        }
        let item = |id: String, key: &str| ViewMenuEntry::item(id, t(key));
        let hit = self.hit(p);
        let editor = self.editor.as_ref()?;
        Some(match hit {
            Some(Hit::Folder(id, _)) => {
                let now = editor.folder(id).and_then(|f| f.color);
                let mut out = vec![
                    item(format!("ds:folder-new:{id}"), "datasources.menu_new_folder")
                        .with_icon("folder"),
                    item(format!("ds:folder-rename:{id}"), "datasources.menu_rename"),
                    self.color_menu(
                        &format!("ds:folder-color:{id}:"),
                        "datasources.color_none",
                        now,
                    ),
                ];
                out.extend(self.move_menu(Item::Folder(id), &format!("ds:folder-move:{id}:")));
                out.push(ViewMenuEntry::Separator);
                out.push(item(format!("ds:folder-delete:{id}"), "datasources.menu_delete_folder"));
                out
            }
            Some(Hit::Row(i)) => {
                let source = &editor.entries.get(i)?.source;
                let id = source.id;
                let now = source.color;
                let mut out = vec![
                    item(format!("ds:source-folder:{id}"), "datasources.menu_new_folder")
                        .with_icon("folder"),
                    self.color_menu(
                        &format!("ds:source-color:{id}:"),
                        "datasources.color_inherit",
                        now,
                    ),
                ];
                out.extend(self.move_menu(Item::Source(id), &format!("ds:source-move:{id}:")));
                out
            }
            Some(Hit::Section(scope)) => {
                let key = match scope {
                    Scope::Project => "project",
                    Scope::Global => "global",
                };
                vec![
                    item(format!("ds:top-folder:{key}"), "datasources.menu_new_folder")
                        .with_icon("folder"),
                ]
            }
            _ => vec![
                item("ds:top-folder:project".into(), "datasources.menu_new_folder")
                    .with_icon("folder"),
            ],
        })
    }

    /// A row of the list's menu chosen.
    pub(super) fn list_command(&mut self, id: &str) -> bool {
        let Some(rest) = id.strip_prefix("ds:") else { return false };
        let (what, arg) = rest.split_once(':').unwrap_or((rest, ""));
        let place = |text: &str| match text {
            "project" => Some(Place::Top(Scope::Project)),
            "global" => Some(Place::Top(Scope::Global)),
            other => FolderId::parse(other).map(Place::Folder),
        };
        let color = |text: &str| if text == "none" { None } else { SourceColor::parse(text) };
        let source = |text: &str| DataSourceId::parse(text);
        let name = t("datasources.new_folder");
        let Some(editor) = &mut self.editor else { return false };
        match what {
            "top-folder" => {
                let Some(place) = place(arg) else { return false };
                editor.add_folder(place, &name);
                self.focus = Some(Field::FolderName);
            }
            "folder-new" => {
                let Some(id) = FolderId::parse(arg) else { return false };
                editor.add_folder(Place::Folder(id), &name);
                self.focus = Some(Field::FolderName);
            }
            "source-folder" => {
                let Some(at) = source(arg)
                    .and_then(|id| editor.entries.iter().position(|e| e.source.id == id))
                else {
                    return false;
                };
                editor.select(Some(at));
                let place = editor.new_folder_place();
                editor.add_folder(place, &name);
                self.focus = Some(Field::FolderName);
            }
            "folder-rename" => {
                let Some(id) = FolderId::parse(arg) else { return false };
                editor.select_folder(id);
                editor.folder_name.select_all();
                self.focus = Some(Field::FolderName);
            }
            "folder-delete" => {
                let Some(id) = FolderId::parse(arg) else { return false };
                editor.delete_folder(id);
                self.focus = None;
            }
            "folder-color" => {
                let Some((id, c)) = arg.split_once(':') else { return false };
                let Some(id) = FolderId::parse(id) else { return false };
                editor.set_folder_color(id, color(c));
            }
            "source-color" => {
                let Some((id, c)) = arg.split_once(':') else { return false };
                let Some(id) = source(id) else { return false };
                if let Some(e) = editor.entries.iter_mut().find(|e| e.source.id == id) {
                    e.source.color = color(c);
                }
            }
            "folder-move" | "source-move" => {
                let Some((id, to)) = arg.split_once(':') else { return false };
                let Some(to) = place(to) else { return false };
                let item = if what == "folder-move" {
                    FolderId::parse(id).map(Item::Folder)
                } else {
                    source(id).map(Item::Source)
                };
                let Some(item) = item else { return false };
                self.move_to(item, to);
            }
            _ => return false,
        }
        self.confirm_close = false;
        self.error = None;
        true
    }
}
