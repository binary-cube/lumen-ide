use super::*;

use crate::branches::BranchItem;
use crate::fake::{FakeRepository, commit, entry};

fn view_with(log: Vec<version_control::LogEntry>) -> LogView {
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log,
        details: vec![CommitDetails {
            commit: commit("m", "merge"),
            email: "ana@x.ro".into(),
            message: "merge\n\nbody".into(),
            parents: vec!["a".into(), "f".into()],
            files: vec![version_control::FileChange {
                path: "src/a.php".into(),
                old_path: None,
                state: FileState::Modified,
            }],
        }],
        texts: vec![
            (("a".into(), "src/a.php".into()), "old".into()),
            (("m".into(), "src/a.php".into()), "new".into()),
        ],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    // Until nothing is being read any more (up to 15 s with every test running).
    for _ in 0..3000 {
        if !view.poll() && !busy(&view) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    view
}

#[test]
fn the_log_is_read_in_the_background_with_its_graph() {
    let view = view_with(vec![entry("m", &["a", "f"]), entry("f", &["a"]), entry("a", &[])]);
    assert_eq!(view.rows().len(), 3);
    assert!(view.complete);
    assert_eq!(view.rows()[1].graph.column, 1);
    assert_eq!(view.rows()[2].graph.from_above, [0, 1]);
}

#[test]
fn selecting_a_commit_reads_its_details_and_a_file_compares_with_the_parent() {
    let mut view = view_with(vec![entry("m", &["a", "f"]), entry("a", &[])]);
    assert!(view.edit_key(ViewerKey::Down));
    assert_eq!(view.selected, Some(0));
    // Until nothing is being read any more (up to 15 s with every test running).
    for _ in 0..3000 {
        if !view.poll() && !busy(&view) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(view.details.as_ref().map(|d| d.files.len()), Some(1));
    let Some(ViewerAction::CompareBeside(input, title)) = compared_file(&mut view) else {
        panic!("no comparison");
    };
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("old", "new"));
    assert_eq!(title, "a.php · m");
    // Esc closes the details.
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(view.details.is_none());
    // Its action (its key, `r`, is the keymap's): read again.
    assert!(view.command("reload"));
    assert!(!view.key('r'));
}

#[test]
fn pages_continue_while_scrolling() {
    let many: Vec<_> = (0..PAGE + 10)
        .map(|i| {
            let parent = format!("c{}", i + 1);
            entry(&format!("c{i}"), &[parent.as_str()])
        })
        .collect();
    let mut view = view_with(many);
    assert_eq!(view.rows().len(), PAGE);
    assert!(!view.complete);
    view.load_more();
    // Until nothing is being read any more (up to 15 s with every test running).
    for _ in 0..3000 {
        if !view.poll() && !busy(&view) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(view.rows().len(), PAGE + 10);
    // One lane all along: the graph continued across pages.
    assert!(view.rows().iter().all(|r| r.graph.column == 0));
}

fn settle(view: &mut LogView) {
    // Until nothing is being read any more (up to 15 s with every test running).
    for _ in 0..3000 {
        if !view.poll() && !busy(view) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_branch_chosen_shows_only_its_history() {
    let mut feature = entry("f", &["a"]);
    feature.refs =
        vec![version_control::RefLabel { name: "feature/x".into(), kind: RefKind::Branch }];
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("m", &["a"]), feature, entry("a", &[])],
        refs: vec![
            version_control::RefLabel { name: "main".into(), kind: RefKind::Branch },
            version_control::RefLabel { name: "feature/x".into(), kind: RefKind::Branch },
        ],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    assert_eq!(view.rows().len(), 3);
    // All, HEAD, Local, feature/, x, main.
    let x = view
        .branch_rows
        .iter()
        .position(|r| matches!(&r.item, BranchItem::Ref { name, .. } if name == "feature/x"))
        .expect("the branch is listed");
    view.choose_branch(x);
    settle(&mut view);
    assert_eq!(view.filter.references, ["feature/x"]);
    assert_eq!(
        view.rows().iter().map(|r| r.entry.commit.id.as_str()).collect::<Vec<_>>(),
        ["f", "a"]
    );
    // A folder closes; "All" brings everything back.
    let folder = view
        .branch_rows
        .iter()
        .position(|r| matches!(r.item, BranchItem::Group { .. }) && r.depth == 1);
    let before = view.branch_rows.len();
    view.choose_branch(folder.unwrap());
    assert_eq!(view.branch_rows.len(), before - 1);
    view.choose_branch(0);
    settle(&mut view);
    assert_eq!(view.rows().len(), 3);
}

#[test]
fn a_files_history_shows_that_file_of_a_commit() {
    let file = |path: &str| version_control::FileChange {
        path: path.into(),
        old_path: None,
        state: FileState::Modified,
    };
    let mut view = LogView::default();
    view.path = Some("src/b.php".into());
    view.details_all = vec![Arc::new(CommitDetails {
        commit: commit("m", "m"),
        email: String::new(),
        message: "m".into(),
        parents: Vec::new(),
        files: vec![file("src/a.php"), file("src/b.php"), file("src/c.php")],
    })];
    view.refresh_files();
    assert_eq!(view.shown_paths(), ["src/b.php"]);
    view.all_files = true;
    view.refresh_files();
    assert_eq!(view.shown_paths(), ["src/a.php", "src/b.php", "src/c.php"]);
}

#[test]
fn a_commit_asked_for_is_selected_once_read_past_the_first_page() {
    // A straight history longer than a page; the commit asked for is on the second one.
    let ids: Vec<String> = (0..PAGE + 10).map(|i| format!("c{i}")).collect();
    let log = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let parents: Vec<&str> = ids.get(i + 1).map(String::as_str).into_iter().collect();
            entry(id, &parents)
        })
        .collect();
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository { log, ..Default::default() });
    let wanted = format!("c{}", PAGE + 3);
    let mut view = LogView::default();
    view.set_data(&LogData {
        repo,
        path: None,
        commit: Some(wanted.clone()),
        queries: None,
        reference: None,
    });
    settle(&mut view);
    let selected = view.selected.map(|i| view.rows()[i].entry.commit.id.clone());
    assert_eq!(selected, Some(wanted));
    assert!(view.wanted.is_none());
    assert!(view.reveal_selected, "brought into view at the next paint");
}

#[test]
fn the_separators_handles_are_drawn_over_the_panels_beside_them() {
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("m", &["a"]), entry("a", &[])],
        refs: vec![version_control::RefLabel { name: "main".into(), kind: RefKind::Branch }],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    assert!(view.edit_key(ViewerKey::Down), "a commit selected: its details shown");
    settle(&mut view);
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1400.0, 600.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert_eq!(view.splitters.len(), 2);
    for splitter in &view.splitters {
        assert!(splitter.is_on_top(&scene), "{:?}", splitter.bounds());
        // In its own seam: grabbing it never takes the list's scrollbar or a row.
        let (s, list) = (splitter.bounds(), view.list);
        assert!(s.max_x() <= list.min_x() || s.min_x() >= list.max_x(), "{s:?} {list:?}");
        assert!(s.min_x() >= view.branches.max_x(), "{s:?} {:?}", view.branches);
    }
}

/// The panels' widths are the project's: let go, they're kept in its sizes; the next log
/// (or the next session's) opens with them.
#[test]
fn the_panels_widths_are_kept_in_the_project_s_sizes() {
    let sizes = workbench_model::LayoutSizes::new();
    let mut view = view_with(vec![entry("a", &[])]);
    view.set_data(&sizes);
    view.bounds = kernel::Rect::new(0.0, 0.0, 1400.0, 600.0);
    view.scale = 1.0;
    // Dragged by the seams' middles: the panels end half a seam before.
    view.drag_separator(0, 220.0 + SEAM / 2.0);
    view.drag_separator(1, 1000.0 - SEAM / 2.0);
    view.keep_widths();
    assert_eq!(sizes.get(BRANCHES_SIZE), Some(220.0));
    assert_eq!(sizes.get(DETAILS_SIZE), Some(400.0));
    let mut next = LogView::default();
    next.set_data(&sizes);
    assert_eq!((next.branches_w, next.details_w), (220.0, 400.0));
}

/// A commit with a long message and many files: the details scroll (the id, the date and the
/// author staying), the whole message then every file reachable, never past the end.
#[test]
fn the_details_scroll_to_the_whole_message_and_every_file() {
    let files: Vec<_> = (0..54)
        .map(|i| version_control::FileChange {
            path: format!("src/file{i}.php"),
            old_path: None,
            state: FileState::Modified,
        })
        .collect();
    let message = (0..40).map(|i| format!("line {i} of a long message")).collect::<Vec<_>>();
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("m", &[])],
        details: vec![CommitDetails {
            commit: commit("m", "a long one"),
            email: String::new(),
            message: format!("a long one\n\n{}", message.join("\n")),
            parents: Vec::new(),
            files,
        }],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    assert!(view.edit_key(ViewerKey::Down));
    settle(&mut view);
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |view: &mut LogView| {
        view.paint(ViewerPaint {
            bounds: kernel::Rect::new(0.0, 0.0, 1400.0, 600.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut render_api::Scene::default(),
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view);
    paint(&mut view);
    // The whole message, never cut short.
    let lines = &view.message.as_ref().expect("wrapped").1;
    assert!(lines.len() >= 42, "{}", lines.len());
    assert!(lines.iter().all(|(l, _)| !l.ends_with('…')));
    assert!(view.detail_scroll.overflows(), "taller than its room");
    // Every file in the tree above (54 of them), which scrolls under the wheel to its end.
    assert_eq!(view.shown_paths().len(), 54);
    let panel = view.details_panel;
    let above = Point::new(panel.min_x() + 40.0, panel.min_y() - 60.0);
    view.hover(above);
    assert!(view.scroll(0.0, 100_000.0));
    assert!(!view.scroll(0.0, 50.0), "the tree never past its end");
    // The wheel over the details scrolls them (the list stays), up to their end.
    view.hover(Point::new(panel.min_x() + 40.0, panel.min_y() + 100.0));
    let list_scroll = view.scroll;
    assert!(view.scroll(0.0, 100_000.0));
    assert_eq!(view.scroll, list_scroll);
    paint(&mut view);
    assert!(!view.scroll(0.0, 50.0), "never past the end");
    // The id stays on top (it doesn't scroll away).
    assert!(view.detail_room.min_y() > panel.min_y() + 40.0);
    // Back up to the top.
    assert!(view.scroll(0.0, -100_000.0));
    assert_eq!(view.detail_scroll.offset(), 0.0);
}

/// A workbench whose Git window records the tabs it was asked for.
struct PanelHost {
    events: kernel::EventBus,
    /// The tabs open, in order; every view shown (its title).
    tabs: Vec<String>,
    shown: Vec<String>,
}

impl WorkbenchHost for PanelHost {
    fn project_root(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn project_id(&self) -> Option<kernel::ProjectId> {
        Some(kernel::ProjectId::new())
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        Some(workbench_model::ActiveFile {
            path: "/p/src/a.php".into(),
            text: "".into(),
            line: 0,
            cursor: 0,
            selection: None,
        })
    }
    fn repository(&self) -> Option<Arc<dyn Repository>> {
        Some(Arc::new(FakeRepository::default()))
    }
    fn project_storage(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, _: workbench_model::Notification) {}
}

impl workbench_model::ViewHost for PanelHost {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
    fn show_in_panel(&mut self, tool: &str, title: &str, _: &str, _: &dyn std::any::Any) -> bool {
        assert_eq!(tool, TOOL);
        if !self.tabs.iter().any(|t| t == title) {
            self.tabs.push(title.to_owned());
        }
        self.shown.push(title.to_owned());
        true
    }
    fn in_panel(&self, _: &str, title: &str) -> bool {
        self.tabs.iter().any(|t| t == title)
    }
    fn show_in_panel_keyed(
        &mut self,
        tool: &str,
        key: &str,
        _: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        self.show_in_panel(tool, key, extension, data)
    }
    fn add_to_panel_keyed(
        &mut self,
        tool: &str,
        key: &str,
        _: &str,
        _: &str,
        _: &dyn std::any::Any,
    ) -> bool {
        assert_eq!(tool, TOOL);
        if !self.tabs.iter().any(|t| t == key) {
            self.tabs.push(key.to_owned());
        }
        true
    }
}

impl workbench_model::EditorHost for PanelHost {}
impl workbench_model::RefactoringHost for PanelHost {}
impl workbench_model::ServicesHost for PanelHost {}
impl workbench_model::SystemHost for PanelHost {}

/// The Git window opens on its Log with the Commit beside it, in this order, before any
/// history; once open, they're only shown. The Changes comes only when asked for.
#[test]
fn the_git_window_opens_with_its_log_and_commit_first() {
    let mut host = PanelHost { events: kernel::EventBus::default(), tabs: vec![], shown: vec![] };
    let log = LOG_KEY.to_owned();
    let commit = crate::commit_view::KEY.to_owned();
    assert!(ShowFileHistory.run(&mut host));
    let history = tr!("git.file_history_tab", name = "a.php");
    assert_eq!(host.tabs, [log.clone(), commit, history.clone()]);
    assert_eq!(host.shown.last(), Some(&history), "the history is the one shown");
    host.shown.clear();
    assert!(ShowLog.run(&mut host));
    assert_eq!(host.shown, [log], "nothing made again");
    assert!(crate::ShowChanges.run(&mut host));
    assert_eq!(host.tabs.len(), 4, "the Changes made when asked for");
}

/// The whole log is a fixed tab; a file's history closes, and so does the Changes (opened on
/// demand, not made again at the next start).
#[test]
fn the_log_is_fixed_the_changes_closes() {
    let mut view = LogView::default();
    assert!(!view.closable());
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository::default());
    view.set_data(&LogData {
        repo,
        path: Some("src/a.php".into()),
        commit: None,
        queries: None,
        reference: None,
    });
    assert!(view.closable());
    let changes = crate::changes_view::ChangesList::default();
    assert!(changes.closable() && !changes.restorable());
}

/// ⇧ / ⌘ choose several commits: their files together (a path once, as the newest left
/// it; added if an older one added it); one left: its own again.
#[test]
fn several_commits_chosen_show_their_files_together() {
    let change = |path: &str, state| version_control::FileChange {
        path: path.into(),
        old_path: None,
        state,
    };
    let details = |id: &str, files| CommitDetails {
        commit: commit(id, id),
        email: String::new(),
        message: id.into(),
        parents: Vec::new(),
        files,
    };
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("c", &["b"]), entry("b", &["a"]), entry("a", &[])],
        details: vec![
            details("c", vec![change("x.php", FileState::Modified)]),
            details(
                "b",
                vec![change("x.php", FileState::Added), change("y.php", FileState::Modified)],
            ),
            details("a", vec![change("z.php", FileState::Added)]),
        ],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    view.select(0);
    settle(&mut view);
    assert_eq!(view.shown_paths(), ["x.php"]);
    let shift = ui_components::PressMods { shift: true, ..Default::default() };
    view.select_with(1, shift);
    settle(&mut view);
    assert_eq!(view.chosen, [0, 1]);
    assert_eq!(view.shown_paths(), ["x.php", "y.php"]);
    assert_eq!(view.files[0].1.state, FileState::Added, "added by the older one");
    assert!(view.details.is_none(), "several: no single commit's details");
    let cmd = ui_components::PressMods { primary: true, ..Default::default() };
    view.select_with(2, cmd);
    settle(&mut view);
    assert_eq!(view.chosen, [0, 1, 2]);
    assert_eq!(view.shown_paths().len(), 3);
    view.select_with(0, cmd);
    view.select_with(1, cmd);
    settle(&mut view);
    assert_eq!(view.chosen, [2]);
    assert_eq!(view.shown_paths(), ["z.php"]);
    assert!(view.details.is_some());
}

/// The separator between the details' files and the commit: dragged, its share kept in the
/// project; a double click back to the default.
#[test]
fn the_details_split_is_dragged_and_kept() {
    let sizes = workbench_model::LayoutSizes::new();
    let mut view = view_with(vec![entry("a", &[])]);
    view.set_data(&sizes);
    view.scale = 1.0;
    view.details_area = kernel::Rect::new(1000.0, 100.0, 400.0, 500.0);
    let default = view.files_height(view.details_area);
    assert_eq!(default, (500.0 * details::FILES_SHARE).round());
    view.drag_files_split(100.0 + 150.0);
    view.keep_widths();
    assert_eq!(view.files_height(view.details_area), 150.0);
    assert_eq!(sizes.get(FILES_SHARE_SIZE), Some(0.3));
    let mut next = LogView::default();
    next.set_data(&sizes);
    assert_eq!(next.files_share, 0.3);
    // Never squeezing either part below its least room.
    view.drag_files_split(100.0 + 495.0);
    assert_eq!(view.files_height(view.details_area), 410.0);
    view.reset_files_split();
    assert_eq!(view.files_height(view.details_area), default);
    assert_eq!(sizes.get(FILES_SHARE_SIZE), Some(details::FILES_SHARE));
}

/// Saved with the session and made again at a start: the log and a file's history (by its
/// path) find their repository again and read only once shown; without one, they go.
#[test]
fn the_log_is_made_again_from_a_session() {
    let mut history = LogView::default();
    history.path = Some("src/a.php".into());
    assert!(history.restorable());
    assert_eq!(history.memento().as_deref(), Some("src/a.php"));
    let here = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let restored = ui_components::Restored {
        project: Some(here),
        memento: Some("src/a.php".into()),
        ..Default::default()
    };
    let mut view = LogView::default();
    view.set_data(&restored);
    assert!(view.repo.is_some() && view.loading.is_none(), "read when shown");
    assert_eq!(view.path.as_deref(), Some("src/a.php"));
    assert!(view.stale && !view.closed());
    let nowhere = std::env::temp_dir().join(format!("no-repo-{}", std::process::id()));
    std::fs::create_dir_all(&nowhere).unwrap();
    let mut lost = LogView::default();
    lost.set_data(&ui_components::Restored {
        project: Some(nowhere.clone()),
        ..Default::default()
    });
    let mut changes = crate::changes_view::ChangesList::default();
    changes.set_data(&ui_components::Restored {
        project: Some(nowhere.clone()),
        ..Default::default()
    });
    let _ = std::fs::remove_dir(&nowhere);
    // A temp folder may sit inside a repository (unlikely): then it's found.
    if git::GitRepository::discover(&std::env::temp_dir()).is_none() {
        assert!(lost.closed() && changes.closed());
    }
}

/// What the view prepared in the background (a comparison being read).
fn prepared(view: &mut LogView) -> Option<ViewerAction> {
    let started = std::time::Instant::now();
    while view.opening.is_busy() && started.elapsed() < std::time::Duration::from_secs(10) {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    view.take_action()
}

fn compared_file(view: &mut LogView) -> Option<ViewerAction> {
    view.compare_file(0);
    prepared(view)
}

/// The Git window made again from a session that lacks its Commit tab (or its Log): the
/// one made again asks, once, for the other — made behind, nothing shown changes.
#[test]
fn a_session_without_a_fixed_tab_gets_it_back_behind_the_one_shown() {
    let mut host = PanelHost {
        events: kernel::EventBus::default(),
        tabs: vec![LOG_KEY.into()],
        shown: vec![],
    };
    let mut view = LogView::default();
    let dir = std::env::temp_dir().join(format!("git-fixed-tabs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = std::process::Command::new("git").args(["init", "-q"]).current_dir(&dir).status();
    if !ok.is_ok_and(|s| s.success()) {
        return;
    }
    view.set_data(&ui_components::Restored {
        project: Some(dir.clone()),
        id: None,
        memento: None,
        queries: None,
    });
    let Some(ViewerAction::WithHost(work)) = view.take_action() else {
        panic!("it asks for the other fixed tabs");
    };
    work.run(&mut host);
    assert_eq!(host.tabs, [LOG_KEY.to_owned(), crate::commit_view::KEY.to_owned()]);
    assert!(host.shown.is_empty(), "nothing shown changes");
    assert!(view.take_action().is_none(), "asked once");
    let _ = std::fs::remove_dir_all(dir);
}

/// Something is still being read in the background (the log, a commit's details, the lists).
fn busy(view: &LogView) -> bool {
    ui_components::Viewer::is_loading(view)
}

/// Paints the log in a wide tab.
fn paint_wide(view: &mut LogView) {
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1400.0, 600.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// The point on the right edge of `lane` near its top where a press makes `held` true
/// (an auto-hiding thumb shown first by a scroll of the view).
fn thumb_in(view: &mut LogView, lane: Rect, held: fn(&LogView) -> bool) -> Option<Point> {
    (0..20).find_map(|dy| {
        (0..16).find_map(|dx| {
            let at =
                Point::new(lane.max_x() - 1.0 - dx as f32, lane.min_y() + 4.0 + dy as f32 * 4.0);
            Viewer::press(view, at, 1);
            if held(view) {
                return Some(at);
            }
            view.release();
            None
        })
    })
}

/// Many commits: the list's thumb pressed and dragged down scrolls it; the separator before
/// the details pressed and dragged left widens them, kept in the project's sizes once let go.
#[test]
fn the_commits_thumb_and_a_separator_are_dragged() {
    let log: Vec<_> = (0..200)
        .map(|i| {
            let parent = format!("c{}", i + 1);
            let parents = [parent.as_str()];
            entry(&format!("c{i}"), if i < 199 { &parents[..] } else { &[] })
        })
        .collect();
    let mut view = view_with(log);
    let sizes = workbench_model::LayoutSizes::new();
    view.set_data(&sizes);
    paint_wide(&mut view);
    // A hidden bar shows once the list scrolls; back at the top after.
    view.hover(Point::new(view.list.min_x() + 40.0, view.list.min_y() + 40.0));
    assert!(view.scroll(0.0, 1.0));
    paint_wide(&mut view);
    assert!(view.scroll(0.0, -1.0));
    paint_wide(&mut view);
    let lane = view.list;
    let at = thumb_in(&mut view, lane, |v| v.list_bar.is_dragging()).expect("the list's thumb");
    assert!(view.drag(Point::new(at.x, at.y + 80.0)));
    assert!(view.scroll > 0.0, "the commits scrolled");
    assert!(view.release());
    assert!(!view.list_bar.is_dragging());
    // The separator between the list and the details, in its seam.
    let seam = view.splitters[1].bounds();
    let grab = Point::new(seam.min_x() + seam.size.width / 2.0, seam.min_y() + 200.0);
    let before = view.details_w;
    view.press(grab, 1);
    assert!(view.splitters[1].is_dragging(), "the separator taken");
    let to = Point::new(grab.x - 80.0, grab.y);
    assert!(view.drag(to));
    assert!(view.details_w > before, "{} > {before}", view.details_w);
    // Dragged, it keeps its cursor wherever the pointer went.
    assert_eq!(view.cursor_at(to), Some(ViewerCursor::ResizeHorizontal));
    assert!(view.release());
    assert_eq!(sizes.get(DETAILS_SIZE), Some(view.details_w.round()));
}

/// A commit with many files: the details' thumb pressed and dragged down scrolls them.
#[test]
fn the_details_thumb_is_dragged() {
    let files: Vec<_> = (0..80)
        .map(|i| version_control::FileChange {
            path: format!("src/file{i}.php"),
            old_path: None,
            state: FileState::Modified,
        })
        .collect();
    let message = (0..60).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("m", &[])],
        details: vec![CommitDetails {
            commit: commit("m", "a long one"),
            email: String::new(),
            message: format!("a long one\n\n{message}"),
            parents: Vec::new(),
            files,
        }],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    assert!(view.edit_key(ViewerKey::Down));
    settle(&mut view);
    paint_wide(&mut view);
    paint_wide(&mut view);
    assert!(view.detail_scroll.overflows());
    // A hidden bar shows once the details scroll; back at the top after.
    let room = view.detail_room;
    view.hover(Point::new(room.min_x() + 40.0, room.min_y() + 40.0));
    assert!(view.scroll(0.0, 1.0));
    paint_wide(&mut view);
    assert!(view.scroll(0.0, -1.0));
    paint_wide(&mut view);
    let at = thumb_in(&mut view, room, |v| v.detail_bar.is_dragging()).expect("the thumb");
    assert!(view.drag(Point::new(at.x, at.y + 60.0)));
    assert!(view.detail_scroll.offset() > 0.0, "the details scrolled");
    assert!(view.release());
}

/// A log whose first commit, selected, changed six files, painted: the files' place.
fn view_with_files() -> (LogView, Rect) {
    let files: Vec<_> = (0..6)
        .map(|i| version_control::FileChange {
            path: format!("src/file{i}.php"),
            old_path: None,
            state: FileState::Modified,
        })
        .collect();
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("m", &[]), entry("n", &[])],
        details: vec![CommitDetails {
            commit: commit("m", "one"),
            email: String::new(),
            message: "one".into(),
            parents: Vec::new(),
            files,
        }],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    assert!(view.edit_key(ViewerKey::Down));
    settle(&mut view);
    paint_wide(&mut view);
    paint_wide(&mut view);
    let (files, _) = *view.hits.iter().find(|(_, h)| *h == Hit::Files).unwrap();
    (view, files)
}

/// The first point down the files' place that is on a file's row.
fn on_a_file(view: &mut LogView, files: Rect) -> Point {
    let x = files.min_x() + 60.0;
    let mut y = files.min_y();
    while y < files.max_y() {
        let p = Point::new(x, y);
        if view.tree.file_at(p).is_some() {
            return p;
        }
        y += 2.0;
    }
    panic!("no file's row in {files:?}");
}

/// The files' tree given the keys by a press on it.
fn press_files(view: &mut LogView, files: Rect) {
    let on = Point::new(files.min_x() + 60.0, files.max_y() - 10.0);
    view.hover(on);
    view.press(on, 1);
    view.release();
    assert!(view.files_focused(), "the tree has the keys");
}

/// The details' files through their controls: a press on the tree gives it the keys (Tab
/// isn't its, ↓ walks its rows), a press on a commit's row takes them back to the commits.
#[test]
fn the_files_tree_takes_the_keys_when_pressed() {
    let (mut view, files) = view_with_files();
    press_files(&mut view, files);
    assert!(!view.edit_key(ViewerKey::Tab), "Tab isn't the tree's");
    assert!(view.edit_key(ViewerKey::Down));
    assert!(view.files_focused());
    let (row, _) = *view.hits.iter().find(|(_, h)| matches!(h, Hit::Row(_))).unwrap();
    view.press(row.center(), 1);
    assert!(!view.files_focused(), "a commit pressed: the commits have the keys");
}

/// The text field typed in, then a file's menu asked for: the tree gets the focus and the
/// typing goes on (the text, the editing keys and every other key are the field's, the
/// tree's selection stays); Esc leaves the field, the tree keeping the focus and its keys.
#[test]
fn a_file_s_menu_asked_for_while_typing_leaves_the_typing_on() {
    let (mut view, files) = view_with_files();
    let (field, _) = *view.hits.iter().find(|(_, h)| *h == Hit::TextField).unwrap();
    view.press(field.center(), 1);
    view.release();
    assert_eq!(view.bar.focus, Some(filters::Field::Text));
    assert!(view.text_input("ab"));
    let on = on_a_file(&mut view, files);
    assert!(view.context_menu(on).is_some(), "the file's menu");
    assert!(view.files_focused(), "the tree has the focus");
    assert!(view.wants_text());
    let chosen = view.tree.selected_file();
    assert!(view.text_input("c"));
    assert_eq!(view.bar.text.text(), "abc", "typing in the bar while the tree is focused");
    assert!(view.edit_key(ViewerKey::Backspace));
    assert_eq!(view.bar.text.text(), "ab");
    assert!(view.edit_key(ViewerKey::Down), "kept by the field");
    assert!(view.edit_key(ViewerKey::Tab), "kept by the field");
    assert_eq!(view.tree.selected_file(), chosen, "the tree's rows stay");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("ab"), "the field's selection");
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.bar.focus, None, "Esc: the field left");
    assert!(view.files_focused(), "the tree keeps the focus");
    assert!(view.edit_key(ViewerKey::Down), "the tree's keys again");
    assert_ne!(view.tree.selected_file(), chosen, "↓ walked its rows");
    assert!(!view.text_input("d"), "the tree isn't searching: nothing takes the text");
    assert_eq!(view.bar.text.text(), "ab");
}

/// A filter's list opened from its command while the tree has the focus: the list's search
/// takes the keys and the text, the tree keeps the focus; Esc closes the list, the tree
/// still focused.
#[test]
fn a_filter_s_list_opened_while_the_tree_is_focused() {
    let (mut view, files) = view_with_files();
    press_files(&mut view, files);
    assert!(view.command("filter:branch"));
    assert_eq!(view.bar.focus, Some(filters::Field::Query), "its search typed in");
    assert!(view.files_focused());
    assert!(view.text_input("ma"));
    assert!(view.edit_key(ViewerKey::Backspace));
    assert_eq!(view.bar.popup.as_ref().unwrap().query.text(), "m");
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(view.bar.popup.is_none(), "Esc: the list closes");
    assert_eq!(view.bar.focus, None);
    assert!(view.files_focused(), "the tree keeps the focus");
}

/// The files' search typed in, then a filter's list opened from its command: the text typed
/// (and pasted) goes on to the files' search while the keys are the list's; the tree not
/// focused any more, the text is the bar's field's again.
#[test]
fn the_text_goes_to_the_files_search_while_it_searches() {
    let (mut view, files) = view_with_files();
    press_files(&mut view, files);
    view.tree.searching = true;
    assert!(view.command("filter:branch"));
    assert!(view.text_input("s"));
    assert!(view.paste("t"));
    assert_eq!(view.tree.query.text(), "st", "the files' search has the text");
    let popup = view.bar.popup.as_mut().unwrap();
    assert!(popup.query.text().is_empty());
    popup.query.set_text("ma");
    popup.query.input.end(false);
    assert!(view.edit_key(ViewerKey::Backspace));
    assert_eq!(view.bar.popup.as_ref().unwrap().query.text(), "m", "the keys are the list's");
    assert_eq!(view.tree.query.text(), "st");
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(view.bar.popup.is_none());
    assert!(view.files_focused() && view.tree.searching, "the tree keeps searching");
    // The text field pressed into: the tree loses the focus, the text is the field's.
    let (field, _) = *view.hits.iter().find(|(_, h)| *h == Hit::TextField).unwrap();
    view.press(field.center(), 1);
    view.release();
    assert!(!view.files_focused());
    assert!(view.text_input("x"));
    assert_eq!(view.bar.text.text(), "x");
    assert_eq!(view.tree.query.text(), "st");
}

/// Paints the log in a wide view.
fn painted(view: &mut LogView) {
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1400.0, 600.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// "Text or hash" is the app's search field: its `.*` and `Cc` in the room it keeps on the
/// right, its × left of them (a hand over it); × pressed empties the text and the log shows
/// every commit again.
#[test]
fn the_text_field_s_toggles_sit_right_of_its_clear_button() {
    let mut view = view_with(vec![entry("m", &["a"]), entry("a", &[])]);
    view.bar.text.set_text("zzz");
    view.apply_text();
    settle(&mut view);
    assert!(view.rows().is_empty(), "nothing matches");
    painted(&mut view);
    let hit = |h: Hit| view.hits.iter().find(|(_, x)| *x == h).map(|(r, _)| *r).unwrap();
    let (field, regex, case) = (hit(Hit::TextField), hit(Hit::Regex), hit(Hit::MatchCase));
    let clear = view.bar.text.clear_area().expect("× shown with text");
    assert!(field.contains(clear.center()) && field.contains(case.center()));
    assert!(clear.max_x() <= regex.min_x() && regex.max_x() <= case.min_x(), "× . * Cc");
    view.hover(clear.center());
    assert_eq!(view.cursor_at(clear.center()), Some(ViewerCursor::Pointer));
    view.press(clear.center(), 1);
    view.release();
    assert_eq!(view.bar.text.text(), "", "× empties it");
    settle(&mut view);
    assert_eq!(view.rows().len(), 2, "every commit again");
}
