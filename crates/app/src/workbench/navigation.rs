//! Going from the code to what's related to it: "Go to Implementation" (what implements the
//! type or overrides the method at the cursor), "Go to Type Declaration" (the class of the
//! variable, property or call there), "Type Hierarchy" (the type's supertypes and subtypes,
//! with its diagram a row away). The file's language server is asked first where it does
//! it (its capability): its answer within [`DEFINITION_GRACE`]; nothing, an error, a
//! timeout or a server not ready, the built-in engine's (the index). Several places are
//! listed in the choice list beside the cursor; one is jumped to.

use std::time::Instant;

use code_intelligence::hierarchy::{self, Place, Subtypes, TypeHierarchy};
use code_intelligence::servers::{Polled, Reply, ServedLocation};

use super::servers::DEFINITION_GRACE;
use super::*;

/// What a navigation looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Related {
    Implementations,
    TypeDefinition,
    Hierarchy,
}

/// A server's answer being waited for.
#[derive(Debug)]
enum Asked {
    Locations(Reply<Vec<ServedLocation>>),
    Hierarchy(Reply<Option<TypeHierarchy>>),
}

/// What a server answered (`None` inside: nothing, or no answer).
#[derive(Debug)]
pub(super) enum Answer {
    Locations(Option<Vec<ServedLocation>>),
    Hierarchy(Option<TypeHierarchy>),
}

/// A navigation asked of a server: the document, where, what for, until when it's waited.
#[derive(Debug)]
pub(super) struct AskedNavigation {
    doc: DocumentId,
    at: usize,
    kind: Related,
    asked: Asked,
    grace: Instant,
}

/// A row of a navigation's list: the place it opens, or the command it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NavRow {
    Place(PathBuf, usize),
    Command(command_model::CommandId),
    Nothing,
}

impl Workbench {
    /// Runs navigation `kind` from the active editor's cursor. `true` if something was
    /// shown, jumped to or asked.
    pub fn go_to_related(&mut self, text: &mut TextSystem, kind: Related) -> bool {
        let Some(head) = self.tabs.get(self.active).map(|t| t.doc.selections().primary().head)
        else {
            return false;
        };
        if self.served_at(head).uses_server() && self.ask_served_navigation(head, kind) {
            return true;
        }
        self.built_in_navigation(text, head, kind)
    }

    /// Asks the active document's server for `kind` at `head`: `true` if it's asked.
    fn ask_served_navigation(&mut self, head: usize, kind: Related) -> bool {
        let Some(doc) = self.tabs.get(self.active).map(|t| t.doc.id()) else { return false };
        let asked = match kind {
            Related::Implementations => {
                self.ask_server(head, |s, at| s.implementations(at)).map(Asked::Locations)
            }
            Related::TypeDefinition => {
                self.ask_server(head, |s, at| s.type_definition(at)).map(Asked::Locations)
            }
            Related::Hierarchy => {
                self.ask_server(head, |s, at| s.type_hierarchy(at)).map(Asked::Hierarchy)
            }
        };
        let Some(asked) = asked else { return false };
        let grace = Instant::now() + navigation_grace();
        self.data.served.navigation = Some(AskedNavigation { doc, at: head, kind, asked, grace });
        true
    }

    /// The server's navigation answer, if it came. `true` if it did (a redraw).
    pub(super) fn poll_navigation(&mut self) -> bool {
        let Some(asked) = self.data.served.navigation.as_mut() else { return false };
        let answer = match &mut asked.asked {
            Asked::Locations(reply) => match reply.poll() {
                Polled::Waiting => return false,
                Polled::Ready(found) => Answer::Locations(Some(found)),
                Polled::Gone => Answer::Locations(None),
            },
            Asked::Hierarchy(reply) => match reply.poll() {
                Polled::Waiting => return false,
                Polled::Ready(found) => Answer::Hierarchy(found),
                Polled::Gone => Answer::Hierarchy(None),
            },
        };
        let Some(asked) = self.data.served.navigation.take() else { return false };
        self.data.served.navigation_answer = Some((asked.doc, asked.at, asked.kind, answer));
        true
    }

    /// When the waited navigation's grace ends.
    pub(super) fn navigation_deadline(&self) -> Option<Instant> {
        self.data.served.navigation.as_ref().map(|a| a.grace)
    }

    /// A navigation whose grace ended unanswered: the server's question dropped (never a
    /// second list or jump), the built-in engine's answer.
    pub(super) fn navigation_after_grace(&mut self, text: &mut TextSystem) {
        let due = self.data.served.navigation.as_ref().is_some_and(|a| a.grace <= Instant::now());
        if !due {
            return;
        }
        let Some(asked) = self.data.served.navigation.take() else { return };
        if self.tabs.get(self.active).map(|t| t.doc.id()) == Some(asked.doc) {
            self.built_in_navigation(text, asked.at, asked.kind);
        }
    }

    /// The server's answer, used: its places, its hierarchy — or, none, the built-in
    /// engine's.
    pub(super) fn use_navigation_answer(&mut self, text: &mut TextSystem) {
        let Some((doc, at, kind, answer)) = self.data.served.navigation_answer.take() else {
            return;
        };
        if self.tabs.get(self.active).map(|t| t.doc.id()) != Some(doc) {
            return;
        }
        match answer {
            Answer::Locations(Some(found)) if !found.is_empty() => {
                let places = found.into_iter().map(served_place).collect();
                self.show_places(text, kind, String::new(), places);
            }
            Answer::Hierarchy(Some(h)) => {
                self.show_hierarchy(text, h);
            }
            _ => {
                self.built_in_navigation(text, at, kind);
            }
        }
    }

    // ─── The built-in engine's ───

    fn built_in_navigation(&mut self, text: &mut TextSystem, head: usize, kind: Related) -> bool {
        match kind {
            Related::Implementations => {
                let Some((subject, found)) = self.built_in_implementations(head) else {
                    self.show_notification(workbench_model::Notification::info(i18n::t(
                        "navigation.no_symbol",
                    )));
                    return false;
                };
                self.show_places(text, kind, subject, found)
            }
            Related::TypeDefinition => match self.built_in_type_definition(head) {
                Some(place) => self.show_places(text, kind, String::new(), vec![place]),
                None => {
                    self.show_notification(workbench_model::Notification::info(i18n::t(
                        "navigation.no_type",
                    )));
                    false
                }
            },
            Related::Hierarchy => match self.built_in_hierarchy() {
                Some(h) => self.show_hierarchy(text, h),
                None => {
                    self.show_notification(workbench_model::Notification::info(i18n::t(
                        "navigation.no_type",
                    )));
                    false
                }
            },
        }
    }

    /// The declaration at `head`: the one whose name is there, else the one the name there
    /// refers to — its file and itself.
    pub(super) fn symbol_at(
        &mut self,
        head: usize,
    ) -> Option<(PathBuf, code_intelligence::Symbol)> {
        self.refresh_symbols_now();
        let sigil = self.intelligence_at(head).and_then(|i| i.naming().sigil);
        let path = Self::index_path(self.tabs.get(self.active)?);
        {
            let index = self.data.symbols.read().ok()?;
            if let Some(s) =
                index.file(&path).and_then(|f| hierarchy::declaration_at(f, head, sigil))
            {
                return Some((path, s.clone()));
            }
        }
        let target = self.definition_at(head)?;
        let index = self.data.symbols.read().ok()?;
        let s = index.file(&target.path)?.symbols.iter().find(|s| s.offset == target.offset)?;
        Some((target.path.clone(), s.clone()))
    }

    /// What implements the type, or overrides the method, at `head` (its name, the places).
    pub(super) fn built_in_implementations(&mut self, head: usize) -> Option<(String, Vec<Place>)> {
        let naming = self.intelligence_at(head).map(|i| i.naming()).unwrap_or_default();
        let (_, symbol) = self.symbol_at(head)?;
        let index = self.data.symbols.read().ok()?;
        let subtypes = Subtypes::of(&index);
        let found = match (symbol.kind, &symbol.container) {
            (k, _) if k.is_type() => subtypes.implementations(&symbol.fqn),
            (code_intelligence::SymbolKind::Method, Some(container)) => {
                subtypes.overrides(&index, container, &symbol.name, &naming)
            }
            _ => return None,
        };
        Some((symbol.name.to_string(), found))
    }

    /// Where the type of what's at `head` is declared: a variable's (as written for it), a
    /// property's, what a method or a function returns; a type is its own.
    pub(super) fn built_in_type_definition(&mut self, head: usize) -> Option<Place> {
        let intel = self.intelligence_at(head)?;
        let naming = intel.naming();
        let path = Self::index_path(self.tabs.get(self.active)?);
        let source = self.tabs.get(self.active)?.text();
        // `(file, written, at)`: the type as written, in which file and where (its imports).
        let written = match intel.type_at(&source, head) {
            Some(written) => Some((path, written, head, None)),
            None => {
                let (declared_in, symbol) = self.symbol_at(head)?;
                if symbol.kind.is_type() {
                    return Some(Place::of(&declared_in, &symbol));
                }
                let written = hierarchy::declared_type(&symbol)?.to_owned();
                Some((
                    declared_in,
                    written,
                    symbol.offset,
                    symbol.container.as_deref().map(str::to_owned),
                ))
            }
        };
        let (file, written, at, container) = written?;
        let class = hierarchy::class_in_type(&written)?;
        let index = self.data.symbols.read().ok()?;
        let fqn =
            if naming.own_type.iter().any(|o| o.eq_ignore_ascii_case(class)) || class == "$this" {
                container.or_else(|| {
                    (index.file(&file)?.type_at(at).map(|t| t.fqn.clone())).map(String::from)
                })?
            } else {
                index.file(&file)?.resolve_class(class, at, &naming)
            };
        let (p, s) = index.type_by_fqn(&fqn)?;
        Some(Place::of(p, s))
    }

    /// The hierarchy of the type at the cursor (named there, or the one it's in).
    pub(super) fn built_in_hierarchy(&mut self) -> Option<TypeHierarchy> {
        let fqn = self.type_at_cursor()?;
        let naming = self.intelligence().map(|i| i.naming()).unwrap_or_default();
        let index = self.data.symbols.read().ok()?;
        hierarchy::type_hierarchy(&index, &fqn, &naming)
    }

    // ─── Shown ───

    /// `places` found for `kind` (about `subject`): none said, one jumped to, more listed.
    fn show_places(
        &mut self,
        text: &mut TextSystem,
        kind: Related,
        subject: String,
        places: Vec<Place>,
    ) -> bool {
        let reachable: Vec<Place> = places.into_iter().filter(|p| p.path.is_some()).collect();
        match reachable.as_slice() {
            [] => {
                let key = match kind {
                    Related::Implementations => "navigation.no_implementations",
                    _ => "navigation.no_type",
                };
                self.show_notification(workbench_model::Notification::info(i18n::t(key)));
                false
            }
            [one] => {
                let (path, offset) = (one.path.clone().unwrap_or_default(), one.offset);
                self.jump_to_place(text, &path, offset)
            }
            _ => {
                let rows: Vec<(ui_workbench::Choice, NavRow)> = reachable
                    .iter()
                    .map(|p| {
                        (
                            self.place_choice(p, 0, 0),
                            NavRow::Place(p.path.clone().unwrap_or_default(), p.offset),
                        )
                    })
                    .collect();
                let title = i18n::t(match kind {
                    Related::Implementations => "navigation.implementations",
                    _ => "navigation.types",
                });
                self.open_navigation_list(text, (&title, &subject, "arrow-down"), rows)
            }
        }
    }

    /// The hierarchy listed: the supertypes above (farther up, more indented), the type,
    /// the subtypes below, then "Show Diagram".
    fn show_hierarchy(&mut self, text: &mut TextSystem, h: TypeHierarchy) -> bool {
        let mut rows: Vec<(ui_workbench::Choice, NavRow)> = Vec::new();
        for (side, depth, place) in h.rows() {
            let group = (side + 1) as u8;
            let target = match &place.path {
                Some(path) => NavRow::Place(path.clone(), place.offset),
                None => NavRow::Nothing,
            };
            let mut choice = self.place_choice(place, depth, group);
            if side == 0 {
                choice.icon = "crosshair";
            }
            rows.push((choice, target));
        }
        let diagram =
            self.support.commands.iter().find(|c| c.spec.id == workbench_model::SHOW_TYPE_DIAGRAM);
        if let Some(diagram) = diagram {
            let id = diagram.spec.id;
            rows.push((
                ui_workbench::Choice {
                    icon: diagram.icon.unwrap_or("layers"),
                    label: i18n::t("navigation.show_diagram"),
                    value: String::new(),
                    shortcut: crate::commands::shortcut_label(id),
                    group: 3,
                },
                NavRow::Command(id),
            ));
        }
        let title = i18n::t("navigation.hierarchy");
        let subject = h.focus.name.clone();
        // The type itself chosen first (its supertypes above it).
        let focus = h.rows().iter().position(|(side, ..)| *side == 0).unwrap_or(0);
        let open = self.open_navigation_list(text, (&title, &subject, "layers"), rows);
        self.choice_menu.step(focus as isize);
        open
    }

    /// A place as a row of the list: its kind's icon, its name (indented `depth` levels),
    /// where it is.
    fn place_choice(&self, place: &Place, depth: usize, group: u8) -> ui_workbench::Choice {
        use code_intelligence::SymbolKind as K;
        let icon = match place.kind {
            K::Interface => "layers",
            K::Trait => "file-code",
            K::Enum => "rows",
            K::Method | K::Function => "parameter",
            _ => "braces",
        };
        let name = match (&place.container, place.kind) {
            (Some(container), K::Method) => {
                format!("{}::{}", code_intelligence::short_name(container), place.name)
            }
            _ => place.name.clone(),
        };
        let label = format!("{}{name}", "    ".repeat(depth));
        let value = match &place.path {
            Some(path) => {
                // Relative to the project; a file outside it by its name.
                let shown = self
                    .project
                    .as_ref()
                    .and_then(|p| path.strip_prefix(p.root_path()).ok())
                    .or_else(|| path.file_name().map(Path::new))
                    .unwrap_or(path);
                format!("{}:{}", shown.display(), place.line + 1)
            }
            None => place.fqn.clone(),
        };
        ui_workbench::Choice { icon, label, value, shortcut: None, group }
    }

    /// A module's places to choose from (a key's `.env` files, in the order they're loaded),
    /// listed beside the cursor.
    pub(super) fn choose_place(
        &mut self,
        text: &mut TextSystem,
        title: &str,
        places: Vec<code_intelligence::AssistPlace>,
    ) -> bool {
        let rows = places
            .into_iter()
            .map(|p| {
                let choice = ui_workbench::Choice {
                    icon: "file",
                    label: p.label,
                    value: p.detail,
                    shortcut: None,
                    group: 0,
                };
                (choice, NavRow::Place(p.at.path, p.at.offset))
            })
            .collect();
        self.open_navigation_list(text, (title, "", "arrow-down"), rows)
    }

    /// Opens the choice list beside the cursor with `rows` (header: title, subject, icon).
    fn open_navigation_list(
        &mut self,
        text: &mut TextSystem,
        header: (&str, &str, &'static str),
        rows: Vec<(ui_workbench::Choice, NavRow)>,
    ) -> bool {
        let anchor = self.caret_rect(text).map(|r| Point::new(r.min_x(), r.max_y()));
        let (choices, targets): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        self.code_hover.hide();
        self.completion.close();
        let action = i18n::t("navigation.open");
        self.choice_menu.open((header.0, header.1, header.2, &action), choices, anchor);
        self.choice_purpose = ChoicePurpose::Navigate(targets);
        self.choice_menu.is_open()
    }

    /// Opens `path` (if it isn't the active file) with the cursor at `offset`.
    pub(super) fn jump_to_place(
        &mut self,
        text: &mut TextSystem,
        path: &Path,
        offset: usize,
    ) -> bool {
        let here = self.tabs.get(self.active).map(Self::index_path);
        if here.as_deref() != Some(path)
            && let Err(e) = self.open_file(path)
        {
            tracing::warn!("can't open {}: {e}", path.display());
            return false;
        }
        self.focus = Focus::Editor;
        self.jump(text, |doc| commands::click(doc, offset.min(doc.buffer().len()), false, false))
    }

    /// A row of a navigation's list chosen: its place opened (the cursor put there at the
    /// next frame), its command run.
    pub(super) fn navigation_picked(&mut self, row: NavRow) {
        match row {
            NavRow::Place(path, offset) => {
                self.pending_offset = Some((path, offset));
            }
            NavRow::Command(id) => {
                let _ = self.run_command(id);
            }
            NavRow::Nothing => {}
        }
    }

    /// The place a list's row asked for: opened, the cursor there (needs the text system).
    pub(super) fn go_to_pending_offset(&mut self, text: &mut TextSystem) {
        if let Some((path, offset)) = self.pending_offset.take() {
            self.jump_to_place(text, &path, offset);
        }
    }
}

#[cfg(feature = "snapshot")]
impl Workbench {
    /// `inserted` put at the cursor as one edit (snapshots).
    pub fn insert_at_cursor(&mut self, text: &mut TextSystem, inserted: &str) -> bool {
        self.edit(text, |doc| commands::insert_text(doc, inserted))
    }

    /// The cursor right after the first occurrence of `needle` (snapshots).
    pub fn cursor_after_first(&mut self, text: &mut TextSystem, needle: &str) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Some(start) = tab.doc.buffer().text().find(needle) else { return false };
        let at = start + needle.len();
        self.focus = Focus::Editor;
        self.edit(text, |doc| commands::click(doc, at, false, false))
    }
}

/// A server's place: named after its file and line (its symbol isn't said).
fn served_place(l: ServedLocation) -> Place {
    let name = l.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    Place {
        fqn: name.clone(),
        name,
        kind: code_intelligence::SymbolKind::Class,
        container: None,
        path: Some(l.path),
        offset: l.range.start,
        line: l.line,
    }
}

/// How long a navigation waits for the server: [`DEFINITION_GRACE`] (longer in the tests of
/// a server's chained answers, which a loaded machine may slow past it).
fn navigation_grace() -> std::time::Duration {
    #[cfg(test)]
    if let Some(grace) = TEST_GRACE.get() {
        return grace;
    }
    DEFINITION_GRACE
}

#[cfg(test)]
thread_local! {
    /// A test's own navigation grace (`None`: the app's): its thread's only, so the tests
    /// beside it keep theirs.
    pub(super) static TEST_GRACE: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/navigation.rs"]
mod tests;
