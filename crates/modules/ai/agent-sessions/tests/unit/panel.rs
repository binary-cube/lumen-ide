use super::*;

/// An agent of the test: its sessions given, its launches plain.
#[derive(Debug)]
struct Fake(Vec<AgentSession>);

impl AgentProvider for Fake {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn name(&self) -> String {
        "Fake".into()
    }
    fn icon(&self) -> &'static str {
        "sparkles"
    }
    fn detect(&self) -> Option<agents::AgentInstall> {
        None
    }
    fn sessions(&self, _root: &Path, limit: usize) -> Vec<AgentSession> {
        self.0.iter().take(limit).cloned().collect()
    }
    fn new_session(&self, root: &Path) -> agents::Launch {
        agents::Launch::new("fake", &[], Some(root))
    }
    fn resume(&self, session: &AgentSession) -> agents::Launch {
        agents::Launch::new("fake", &["--resume", &session.id], Some(&session.folder))
    }
}

fn session(id: &str, active: i64) -> AgentSession {
    AgentSession {
        agent: "fake",
        id: id.into(),
        title: format!("session {id}"),
        folder: PathBuf::from("/p"),
        started: None,
        active,
        file: None,
    }
}

/// Every agent's sessions, the most recently active first, cut at the limit (saying more).
#[test]
fn the_sessions_come_newest_first_up_to_the_limit() {
    let agents: Vec<Arc<dyn AgentProvider>> =
        vec![Arc::new(Fake(vec![session("a", 10), session("b", 30), session("c", 20)]))];
    let found = gather(&agents, Path::new("/p"), 2, None);
    let ids: Vec<&str> = found.sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["b", "c"]);
    assert!(found.more);
    assert!(!gather(&agents, Path::new("/p"), 5, None).more);
}

/// A click on a session asks for it in a terminal tab of the editor, resumed by its agent,
/// called by its title, with its agent's icon; "+" with one agent starts a new one.
#[test]
fn a_session_is_taken_up_again_in_a_terminal_tab() {
    let fake: Arc<dyn AgentProvider> = Arc::new(Fake(vec![session("a", 10)]));
    let mut panel = SessionsPanel::with_agents(vec![fake]);
    panel.chats = Some(false);
    panel.root = Some(PathBuf::from("/p"));
    panel.sessions = vec![session("a", 10)];
    panel.read_at = Some(Instant::now());
    panel.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    panel.filter();
    let asked = |action: Option<ToolPanelAction>| match action {
        Some(ToolPanelAction::View(ViewerAction::Ask(asked))) => format!("{asked:?}"),
        other => panic!("not a request: {other:?}"),
    };
    assert!(panel.click(Point::new(50.0, 10.0)).is_none(), "opened on letting go");
    assert!(panel.release());
    let text = asked(panel.take_action());
    assert!(text.contains("--resume") && text.contains("\"a\""), "{text}");
    assert!(text.contains("sparkles"), "its agent's icon on its tab: {text}");
    assert!(text.contains("session a") && text.contains("Editor"), "{text}");
    let text = asked(panel.header_action(NEW));
    assert!(text.contains("Fake") && !text.contains("--resume"), "{text}");
    assert!(panel.header().unwrap().iter().any(|b| b.id == NEW));
}

/// With conversations shown, a click still takes the session up in a terminal; its menu
/// opens it as a conversation (the session's id, in the project).
#[test]
fn a_session_opens_as_a_conversation_from_its_menu() {
    let fake: Arc<dyn AgentProvider> = Arc::new(Fake(vec![session("a", 10)]));
    let mut panel = SessionsPanel::with_agents(vec![fake]);
    panel.chats = Some(true);
    panel.root = Some(PathBuf::from("/p"));
    panel.sessions = vec![session("a", 10)];
    panel.read_at = Some(Instant::now());
    panel.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    panel.filter();
    let asked = |action: Option<ToolPanelAction>| match action {
        Some(ToolPanelAction::View(ViewerAction::Ask(asked))) => format!("{asked:?}"),
        other => panic!("not a request: {other:?}"),
    };
    panel.click(Point::new(50.0, 10.0));
    panel.release();
    let text = asked(panel.take_action());
    assert!(text.starts_with("RunInTerminal") && text.contains("--resume"), "{text}");
    let menu = panel.menu_at(Point::new(50.0, 10.0)).unwrap();
    let ids: Vec<&str> = menu
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [OPEN_TERMINAL, OPEN_CHAT, PIN, DONE, UNREAD, FAVORITE, RENAME]);
    assert!(panel.menu_command(OPEN_CHAT));
    let text = asked(panel.take_action());
    assert!(text.starts_with("OpenAgentChat") && text.contains("Some(\"a\")"), "{text}");
}

/// The search narrows the list by every word of the titles, in any case; Esc empties it,
/// then gives the keys back.
#[test]
fn the_search_narrows_the_sessions_by_their_titles() {
    let mut panel = SessionsPanel::with_agents(Vec::new());
    panel.sessions = vec![session("a", 3), session("b", 2), session("c", 1)];
    panel.sessions[0].title = "Matching system".into();
    panel.sessions[1].title = "Advert markdown".into();
    panel.sessions[2].title = "Matching bugs in markdown".into();
    panel.field = Rect::new(0.0, 0.0, 200.0, 30.0);
    panel.filter();
    assert_eq!(panel.shown, [0, 1, 2]);
    panel.click(Point::new(10.0, 10.0));
    assert!(panel.wants_text());
    panel.text_input("MATCHING");
    panel.filter();
    assert_eq!(panel.shown, [0, 2]);
    panel.text_input(" mark");
    panel.filter();
    assert_eq!(panel.shown, [2]);
    assert_eq!(panel.limit, SEARCHED, "all of them looked through");
    assert!(panel.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    panel.filter();
    assert_eq!(panel.shown.len(), 3);
    assert!(panel.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(!panel.wants_text());
}

/// Favorites first, with the missing ones above them; the menu adds or takes off.
#[test]
fn favorites_come_first() {
    let project = kernel::ProjectId::new();
    let mut panel = SessionsPanel::with_agents(Vec::new());
    panel.project = Some(project);
    panel.sessions = vec![session("a", 3), session("b", 2), session("c", 1)];
    panel.read_at = Some(Instant::now());
    panel.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    crate::favorites::toggle(project, "fake", "c", "session c");
    crate::favorites::toggle(project, "fake", "gone", "Old work");
    crate::favorites::seen(project, &[("fake", "a"), ("fake", "b"), ("fake", "c")]);
    panel.filter();
    assert_eq!(panel.missing.len(), 1, "the one gone, first");
    assert_eq!(panel.shown, [2, 0, 1], "the favorite first, then the rest");
    // Row 0: the missing one (no session); row 1: "c".
    let row = |n: f32| Point::new(50.0, n * ROW + 5.0);
    assert_eq!(panel.row_at(row(0.0)), None);
    assert_eq!(panel.missing_at(row(0.0)), Some(0));
    assert_eq!(panel.row_at(row(1.0)), Some(2));
    // Its menu takes "a" in.
    assert!(panel.menu_at(row(2.0)).is_some());
    assert!(panel.menu_command(FAVORITE));
    panel.filter();
    assert_eq!(panel.shown, [0, 2, 1], "both favorites first, in the read's order");
}

/// ⌘A in its search selects what's typed (then typed over); ⌘Z takes the typing back.
#[test]
fn its_search_takes_the_edit_commands() {
    let mut panel = SessionsPanel { typing: true, ..SessionsPanel::default() };
    assert!(panel.text_input("czcz"));
    assert!(ToolPanel::command(&mut panel, "select_all"));
    assert_eq!(panel.selected_text().as_deref(), Some("czcz"));
    assert!(panel.text_input("new"));
    assert_eq!(panel.query.text(), "new");
    assert!(ToolPanel::command(&mut panel, "undo"));
    panel.typing = false;
    assert!(!ToolPanel::command(&mut panel, "select_all"), "not typed in: not its");
}

/// A row's star adds its session to the favorites (and says so in its tooltip), clicked
/// again takes it off, without opening it; the header's star lists only the favorites.
#[test]
fn a_row_s_star_and_the_favorites_filter() {
    let project = kernel::ProjectId::new();
    let mut panel = SessionsPanel::with_agents(Vec::new());
    panel.project = Some(project);
    panel.sessions = vec![session("a", 3), session("b", 2)];
    panel.read_at = Some(Instant::now());
    panel.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    panel.filter();
    // The stars as the rows were drawn: one per row, in one column.
    let star = |row: f32| Rect::new(240.0, row * ROW + 4.0, 24.0, 24.0);
    panel.stars = vec![(star(0.0), Star::Session(0)), (star(1.0), Star::Session(1))];
    let on = |row: f32| Point::new(250.0, row * ROW + 14.0);
    assert_eq!(panel.tooltip_at(on(1.0)).map(|t| t.label), Some(t("agents.favorite")));
    assert!(panel.click(on(1.0)).is_none(), "nothing opened");
    panel.filter();
    assert_eq!(panel.shown, [1, 0], "now a favorite: first");
    // Only the favorites.
    assert!(panel.header_action(ONLY_FAVORITES).is_none());
    panel.filter();
    assert_eq!(panel.shown, [1]);
    assert!(panel.header().unwrap().iter().any(|b| b.icon == crate::FAVORITE_ON_ICON));
    panel.stars = vec![(star(0.0), Star::Session(1))];
    assert_eq!(panel.tooltip_at(on(0.0)).map(|t| t.label), Some(t("agents.unfavorite")));
    assert!(panel.click(on(0.0)).is_none());
    panel.filter();
    assert!(panel.shown.is_empty(), "taken off: none left to list");
}

/// A panel over `sessions` of the fake agent, listed.
fn listed(sessions: Vec<AgentSession>) -> SessionsPanel {
    let fake: Arc<dyn AgentProvider> = Arc::new(Fake(sessions.clone()));
    let mut panel = SessionsPanel::with_agents(vec![fake]);
    panel.chats = Some(false);
    panel.project = Some(kernel::ProjectId::new());
    panel.root = Some(PathBuf::from("/p"));
    panel.sessions = sessions;
    panel.read_at = Some(Instant::now());
    panel.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    panel.filter();
    panel
}

/// The row under the mouse shows its "⋮" (in its age's place): a click opens its menu under
/// it, without opening the session; it says what it is.
#[test]
fn a_row_s_more_button_opens_its_menu() {
    let mut panel = listed(vec![session("a", 3)]);
    let button = Rect::new(270.0, 4.0, 22.0, 22.0);
    panel.more_button = Some((button, 0));
    let on = Point::new(280.0, 14.0);
    assert!(panel.hover(on) && panel.hover_more);
    assert_eq!(panel.tooltip_at(on).map(|t| t.label), Some(t("agents.more_actions")));
    match panel.click(on) {
        Some(ToolPanelAction::MenuUnder { anchor, entries }) => {
            assert_eq!(anchor, button);
            assert!(
                entries.iter().any(|e| matches!(e, ViewMenuEntry::Item { id, .. } if id == DONE))
            );
        }
        other => panic!("not its menu: {other:?}"),
    }
}

/// Marked done: after the others, dimmed; unread: cleared once it's opened; a color label
/// chosen, then back to none — each kept for the project.
#[test]
fn a_session_is_marked_from_its_menu() {
    let mut panel = listed(vec![session("a", 3), session("b", 2)]);
    let project = panel.project.unwrap();
    let row = |n: f32| Point::new(50.0, n * ROW + 5.0);
    panel.menu_at(row(0.0)).unwrap();
    assert!(panel.menu_command(DONE));
    panel.filter();
    assert_eq!(panel.shown, [1, 0], "done: after the others");
    panel.menu_at(row(1.0)).unwrap();
    assert!(panel.menu_command(&format!("{LABEL}green")));
    panel.menu_at(row(1.0)).unwrap();
    assert!(panel.menu_command(UNREAD));
    panel.filter();
    let mark = panel.marked[0].clone().unwrap();
    assert!(mark.done && mark.unread && mark.label == Some(Label::Green));
    // Its menu says how it stands: the label checked, "not done", "read".
    let menu = panel.menu_at(row(1.0)).unwrap();
    let labels = menu.iter().find_map(|e| match e {
        ViewMenuEntry::Submenu { entries, .. } if entries.len() > 4 => Some(entries.clone()),
        _ => None,
    });
    let green = labels.unwrap().into_iter().find_map(|e| match e {
        ViewMenuEntry::Item { id, look, .. } if id == format!("{LABEL}green") => Some(look),
        _ => None,
    });
    assert!(green.is_some_and(|l| l.checked && l.swatch.is_some()));
    // Opened: read.
    panel.resume(0, true);
    panel.filter();
    assert!(!panel.marked[0].as_ref().unwrap().unread);
    panel.menu_at(row(1.0)).unwrap();
    assert!(panel.menu_command(LABEL));
    panel.menu_at(row(1.0)).unwrap();
    assert!(panel.menu_command(DONE));
    panel.filter();
    assert!(crate::favorites::marks(project).is_empty(), "nothing marked: not kept");
}

/// An agent that can't rename its sessions: its menu's "Rename" greyed out. One that can:
/// the title typed in the row, ↵ gives it (shown at once); Esc leaves it as it was.
#[test]
fn a_session_is_renamed_in_its_row() {
    let mut panel = listed(vec![session("a", 3)]);
    let menu = panel.menu_at(Point::new(50.0, 5.0)).unwrap();
    let rename = menu.iter().find_map(|e| match e {
        ViewMenuEntry::Item { id, look, .. } if id == RENAME => Some(*look),
        _ => None,
    });
    assert!(rename.is_some_and(|l| l.disabled), "the fake agent can't rename");
    // Renamed in the row (as its menu would, for an agent that can).
    panel.start_rename(0);
    assert!(panel.wants_text() && panel.takes_keys());
    assert_eq!(panel.selected_text().as_deref(), Some("session a"), "its title, selected");
    assert!(panel.text_input("Matching"));
    assert!(panel.edit_key_with(ViewerKey::Enter, KeyReach::default()));
    assert!(panel.renaming.is_none());
    assert_eq!(panel.sessions[0].title, "Matching", "shown at once");
    assert_eq!(panel.retitled.len(), 1, "kept until the agent's files say it");
    panel.start_rename(0);
    panel.text_input("Other");
    assert!(panel.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert_eq!(panel.sessions[0].title, "Matching", "Esc: as it was");
}

/// "+" starts the agent chosen; its chooser lists the agents, one not installed greyed out,
/// the chosen one checked.
#[test]
fn the_agent_chooser_greys_out_what_can_t_start() {
    #[derive(Debug)]
    struct Missing;
    impl AgentProvider for Missing {
        fn id(&self) -> &'static str {
            "missing"
        }
        fn name(&self) -> String {
            "Missing".into()
        }
        fn icon(&self) -> &'static str {
            "x"
        }
        fn detect(&self) -> Option<agents::AgentInstall> {
            None
        }
        fn installed(&self) -> bool {
            false
        }
        fn sessions(&self, _root: &Path, _limit: usize) -> Vec<AgentSession> {
            Vec::new()
        }
        fn new_session(&self, root: &Path) -> agents::Launch {
            agents::Launch::new("missing", &[], Some(root))
        }
        fn resume(&self, session: &AgentSession) -> agents::Launch {
            agents::Launch::new("missing", &[], Some(&session.folder))
        }
    }
    let agents: Vec<Arc<dyn AgentProvider>> = vec![Arc::new(Missing), Arc::new(Fake(Vec::new()))];
    let mut panel = SessionsPanel::with_agents(agents);
    panel.root = Some(PathBuf::from("/p"));
    let Some(ToolPanelAction::Menu(rows)) = panel.header_action(CHOOSE) else {
        panic!("no chooser");
    };
    let looks: Vec<(String, ui_components::RowLook)> = rows
        .into_iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, look, .. } => Some((id, look)),
            _ => None,
        })
        .collect();
    assert_eq!(looks.len(), 2);
    assert!(looks[0].1.disabled && !looks[0].1.checked, "not installed: greyed out");
    assert!(!looks[1].1.disabled && looks[1].1.checked, "the one \"+\" starts");
    assert!(!panel.menu_command(&format!("{NEW_WITH}missing")), "a greyed-out one never starts");
    assert!(matches!(panel.header_action(NEW), Some(ToolPanelAction::View(_))));
}

/// Pinned: on top of the list, above the favorites; unpinned, back in its place. Its
/// scrollbar hides at rest, as the project's tree's.
#[test]
fn a_pinned_session_comes_first() {
    let mut panel = listed(vec![session("a", 3), session("b", 2), session("c", 1)]);
    let project = panel.project.unwrap();
    crate::favorites::toggle(project, "fake", "b", "session b");
    panel.filter();
    assert_eq!(panel.shown, [1, 0, 2], "the favorite first");
    let row = |n: f32| Point::new(50.0, n * ROW + 5.0);
    panel.menu_at(row(2.0)).unwrap();
    assert!(panel.menu_command(PIN));
    panel.filter();
    assert_eq!(panel.shown, [2, 1, 0], "pinned: above the favorite");
    let menu = panel.menu_at(row(0.0)).unwrap();
    assert!(menu.iter().any(|e| matches!(e, ViewMenuEntry::Item { id, label, .. }
        if id == PIN && *label == t("agents.unpin"))));
    assert!(panel.menu_command(PIN));
    panel.filter();
    assert_eq!(panel.shown, [1, 0, 2]);
    assert!(SessionsPanel::default().bar.next_frame().is_none(), "at rest: no frames");
}

/// Folders group the list: a session moved into one is listed under it (set in), the
/// folder opened or closed by a click; a session dragged onto a folder goes into it, and
/// dragged out of it, out; a search shows a folder only with what's found in it.
#[test]
fn folders_group_the_sessions() {
    let mut panel = listed(vec![session("a", 3), session("b", 2), session("c", 1)]);
    let project = panel.project.unwrap();
    panel.row_h = ROW;
    let row = |n: f32| Point::new(50.0, n * ROW + 5.0);
    // "c" moved into a new folder from its menu: the folder named in its row.
    panel.menu_at(row(2.0)).unwrap();
    assert!(panel.menu_command(MOVE_NEW));
    assert!(panel.renaming.is_some(), "the new folder named in place");
    panel.text_input("Bugs");
    assert!(panel.edit_key_with(ViewerKey::Enter, KeyReach::default()));
    panel.shown_for = None;
    panel.filter();
    assert_eq!(panel.folders[0].name, "Bugs");
    assert_eq!(
        panel.rows,
        [
            Row::Folder(0),
            Row::Session { i: 2, nested: true },
            Row::Session { i: 0, nested: false },
            Row::Session { i: 1, nested: false },
        ]
    );
    assert_eq!(panel.folder_counts, [1]);
    // Closed by a click: its sessions hidden.
    assert!(panel.click(row(0.0)).is_none());
    panel.filter();
    assert_eq!(panel.rows.len(), 3);
    assert!(panel.click(row(0.0)).is_none());
    panel.filter();
    // "a" dragged onto the folder: into it, nothing opened.
    panel.click(row(2.0));
    assert!(panel.drag(row(1.0)));
    assert!(panel.drag(row(0.0)));
    assert_eq!(panel.drop_on, Some(DropOn::Folder(panel.folders[0].id.clone())));
    assert!(panel.release());
    assert!(panel.take_action().is_none(), "dragged: not opened");
    panel.filter();
    assert_eq!(panel.folder_counts, [2]);
    // Dropped on a session in the folder: it stays in it.
    panel.click(row(1.0));
    panel.drag(row(2.0));
    panel.release();
    panel.filter();
    assert_eq!(panel.folder_counts, [2]);
    // "c" dragged out of it, onto the loose rows.
    panel.click(row(1.0));
    panel.drag(row(3.0));
    panel.release();
    panel.filter();
    assert_eq!(panel.folder_counts, [1]);
    // A search: the folder only with what's found in it.
    panel.query = TextInput::new("session b");
    panel.filter();
    assert!(!panel.rows.contains(&Row::Folder(0)), "nothing found in it: left out");
    // Deleted from its menu: its sessions back in the list.
    panel.query = TextInput::default();
    panel.filter();
    panel.menu_at(row(0.0)).unwrap();
    assert!(panel.menu_command(FOLDER_DELETE));
    panel.filter();
    assert!(panel.folders.is_empty() && panel.rows.len() == 3);
    assert!(crate::folders::list(project).is_empty());
}

/// Sessions of two agents: the filter beside the search is offered; choosing one lists only
/// its sessions (a folder holding none of them left out), "All agents" every one's again.
/// With one agent's sessions only, there's nothing to choose: no filter.
#[test]
fn the_list_is_filtered_by_agent() {
    #[derive(Debug)]
    struct Other;
    impl AgentProvider for Other {
        fn id(&self) -> &'static str {
            "other"
        }
        fn name(&self) -> String {
            "Other".into()
        }
        fn icon(&self) -> &'static str {
            "x"
        }
        fn detect(&self) -> Option<agents::AgentInstall> {
            None
        }
        fn sessions(&self, _root: &Path, _limit: usize) -> Vec<AgentSession> {
            Vec::new()
        }
        fn new_session(&self, root: &Path) -> agents::Launch {
            agents::Launch::new("other", &[], Some(root))
        }
        fn resume(&self, session: &AgentSession) -> agents::Launch {
            agents::Launch::new("other", &[], Some(&session.folder))
        }
    }
    let mut panel = listed(vec![session("a", 3), session("b", 2)]);
    panel.icons = vec![("fake", "sparkles"), ("other", "x")];
    assert!(!panel.filters_agents(), "one agent's sessions: nothing to choose");
    panel.agents = Some(vec![Arc::new(Fake(Vec::new())), Arc::new(Other)]);
    let mut theirs = session("c", 1);
    theirs.agent = "other";
    panel.sessions.push(theirs);
    panel.generation += 1;
    panel.filter();
    assert_eq!(panel.shown, [0, 1, 2]);
    assert!(panel.filters_agents());
    let rows: Vec<(String, bool)> = panel
        .agent_filter_menu()
        .into_iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, look, .. } => Some((id, look.checked)),
            _ => None,
        })
        .collect();
    let every = AGENT_FILTER.to_owned();
    let fake = format!("{AGENT_FILTER}fake");
    let other = format!("{AGENT_FILTER}other");
    assert_eq!(rows, [(every.clone(), true), (fake, false), (other.clone(), false)]);
    assert!(panel.menu_command(&other));
    panel.filter();
    assert_eq!(panel.shown, [2], "only the other agent's");
    assert_eq!(panel.agent_name("other"), "Other");
    assert!(panel.menu_command(&every));
    panel.filter();
    assert_eq!(panel.shown, [0, 1, 2], "every agent's again");
}
