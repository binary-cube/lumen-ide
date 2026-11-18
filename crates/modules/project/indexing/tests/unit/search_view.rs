use super::*;
use code_intelligence::{ClassDiagram, SymbolHit, SymbolIndex, SymbolQuery};
use text_search::Hit;
use workspace::{FileFilter, FileMatch, PathResults};

/// A project (at its root: each test its own, the recent searches are kept per project)
/// that finds `needle` in `src/a.php` (line 3), the folder `src` and the file `a.php`.
#[derive(Debug)]
struct Fake(&'static str);

impl ProjectQueries for Fake {
    fn paths(&self, query: &PathQuery<'_>) -> PathResults {
        let folders = if "src".contains(query.text) {
            vec![FileMatch { path: "src".into(), score: 1, name_positions: Vec::new() }]
        } else {
            Vec::new()
        };
        // The letters typed, found in the file's own name (as the real search marks them).
        let files = match "a.php".find(query.text) {
            Some(at) if query.files > 0 => {
                let name_positions = (at..at + query.text.len()).collect();
                vec![FileMatch { path: "src/a.php".into(), score: 1, name_positions }]
            }
            _ => Vec::new(),
        };
        PathResults { folders, files }
    }
    fn symbols(&self, _: &SymbolQuery<'_>) -> Vec<SymbolHit> {
        Vec::new()
    }
    fn text(&self, query: &SearchQuery) -> Vec<Hit> {
        let inside =
            query.paths.is_empty() || query.paths.iter().any(|p| "src/a.php".starts_with(p));
        if query.text != "needle" || !inside {
            return Vec::new();
        }
        vec![Hit {
            path: "src/a.php".into(),
            start_line: 2,
            end_line: 3,
            preview: "<?php\n$needle = 1;".into(),
            score: 1.0,
        }]
    }
    fn files(&self, filter: &FileFilter) -> Vec<PathBuf> {
        let file = "src/a.php";
        if filter.accepts(file) { vec![self.root().join(file)] } else { Vec::new() }
    }
    fn read_source(&self, relative: &Path) -> Option<String> {
        (relative == Path::new("src/a.php")).then(|| "<?php\n$needle = 1;\n".to_owned())
    }
    fn class_diagram(&self, _: &str) -> Option<ClassDiagram> {
        None
    }
    fn root(&self) -> PathBuf {
        PathBuf::from(self.0)
    }
    fn file_list(&self) -> Arc<Vec<String>> {
        Arc::default()
    }
    fn symbol_index(&self) -> SymbolIndex {
        SymbolIndex::default()
    }
    fn language_of(&self, _: &Path) -> String {
        String::new()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn list_dir(&self, _: &Path) -> std::io::Result<Vec<workspace::DirEntry>> {
        Ok(Vec::new())
    }
    fn file_icon(&self, _: &str) -> (&'static str, String) {
        ("file", String::new())
    }
    fn complete_folder(&self, typed: &str, limit: usize) -> Vec<String> {
        let folders = ["lib", "src", "src/sub"].map(String::from);
        workspace::complete_folders(&folders, typed, limit)
    }
}

fn opened(project: kernel::ProjectId, root: &'static str, selection: Option<&str>) -> SearchView {
    let mut view = SearchView::default();
    view.set_data(&SearchData {
        project,
        queries: Arc::new(Fake(root)),
        semantic: None,
        selection: selection.map(str::to_owned),
        folder: None,
        replace: false,
    });
    view
}

#[test]
fn a_result_opens_at_its_line_and_the_search_is_kept() {
    let project = kernel::ProjectId::new();
    let mut view = opened(project, "/result", None);
    view.text_input("needle");
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::OpenAt { path: PathBuf::from("/result/src/a.php"), line: 3 })
    );
    assert!(view.closed());
    // Opened again: the search is among the recent ones — that project's only.
    let view = opened(project, "/result", None);
    assert_eq!(view.panel.recent(0), Some("needle"));
    let other = opened(kernel::ProjectId::new(), "/result", None);
    assert_eq!(other.panel.recent(0), None, "the same folder opened again: another project");
    // Closed: what was kept for it goes.
    forget(project);
    assert_eq!(opened(project, "/result", None).panel.recent(0), None);
}

#[test]
fn a_folder_shows_in_the_tree_and_esc_gives_up() {
    let mut view = opened(kernel::ProjectId::new(), "/folder", Some("src"));
    assert_eq!(view.panel.input.text(), "src", "the editor's selection is searched");
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::ShowFolder(PathBuf::from("/folder/src"))))
    );
    let mut view = opened(kernel::ProjectId::new(), "/folder", None);
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
}

/// `a.php:12:5`: the files by that name, opened at the line and column as written.
#[test]
fn a_file_and_line_opens_there() {
    let mut view = opened(kernel::ProjectId::new(), "/place", None);
    view.text_input("a.php:12:5");
    assert_eq!(
        view.panel.result(0).map(|h| (h.path.as_str(), h.start_line)),
        Some(("src/a.php", 11))
    );
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::OpenFile {
            path: PathBuf::from("/place/src/a.php"),
            line: Some(12),
            column: Some(5)
        })
    );
    // Without a column: its start.
    let mut view = opened(kernel::ProjectId::new(), "/place", None);
    view.text_input("a.php:7");
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::OpenFile {
            path: PathBuf::from("/place/src/a.php"),
            line: Some(7),
            column: Some(1)
        })
    );
}

/// A file found by its name comes first, in its own section, and opens whole; what the
/// files hold comes after.
#[test]
fn a_file_by_its_name_comes_first_and_opens() {
    let mut view = opened(kernel::ProjectId::new(), "/named", None);
    view.text_input("a.php");
    assert_eq!(view.panel.named(0).map(|m| m.path.as_str()), Some("src/a.php"));
    assert_eq!(view.panel.selected_action(), Some(ProjectSearchAction::OpenNamed(0)));
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::OpenFile {
            path: PathBuf::from("/named/src/a.php"),
            line: None,
            column: None
        })
    );
    // Only some of its letters in its name (the rest in its folders): not by name.
    let mut view = opened(kernel::ProjectId::new(), "/named", None);
    view.text_input("needle");
    assert_eq!(view.panel.named(0), None);
    assert_eq!(view.panel.selected_action(), Some(ProjectSearchAction::Open(0)));
}

fn opened_in(folder: &str) -> SearchView {
    let mut view = SearchView::default();
    view.set_data(&SearchData {
        project: kernel::ProjectId::new(),
        queries: Arc::new(Fake("/p")),
        semantic: None,
        selection: None,
        folder: Some(folder.to_owned()),
        replace: false,
    });
    view
}

/// Opened on a folder (its "Find in Files…"): its row shows, and only what's in it is found —
/// the text and the names.
#[test]
fn a_folder_keeps_the_search_in_it() {
    let mut view = opened_in("lib");
    assert!(view.panel.scope_open() && !view.panel.in_scope());
    view.text_input("needle");
    assert!(view.panel.result(0).is_none(), "src/a.php is outside lib");
    let mut view = opened_in("src");
    view.text_input("needle");
    assert_eq!(view.panel.result(0).map(|h| h.path.as_str()), Some("src/a.php"));
    view.panel.input.set_text("");
    view.text_input("a.php");
    assert_eq!(view.panel.named(0).map(|f| f.path.as_str()), Some("src/a.php"));
    assert!(view.panel.folder(0).is_none(), "the folder searched in isn't among what it holds");
}

/// The folder typed with the keyboard: Tab shows its row, the folders it goes on to are
/// suggested, Enter takes one (and goes into it), Esc closes them before the search, Tab goes
/// on to the mask, then back to the search's field.
#[test]
fn a_folder_is_typed_with_its_suggestions() {
    let mut view = opened(kernel::ProjectId::new(), "/q", None);
    assert!(!view.panel.scope_open());
    view.edit_key(ViewerKey::Tab);
    assert!(view.panel.scope_open() && view.panel.in_scope());
    view.text_input("s");
    assert!(view.panel.has_suggestions());
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.panel.scope.text(), "src/");
    assert!(view.panel.has_suggestions(), "its own folders: src/sub");
    view.edit_key(ViewerKey::Escape);
    assert!(!view.panel.has_suggestions() && !view.closed(), "Esc closed only the suggestions");
    view.edit_key(ViewerKey::Tab);
    assert!(view.panel.in_mask(), "from the folder to the mask");
    view.edit_key(ViewerKey::Tab);
    assert!(!view.panel.in_scope() && !view.panel.in_mask());
    view.text_input("needle");
    assert_eq!(view.panel.input.text(), "needle", "typed in the search, not the folder");
    assert_eq!(view.panel.result(0).map(|h| h.path.as_str()), Some("src/a.php"));
}

/// A click anywhere but on the folders suggested (or their field) closes their list; the
/// search stays open.
#[test]
fn a_click_elsewhere_closes_the_folders_suggested() {
    let mut view = opened(kernel::ProjectId::new(), "/q2", None);
    view.edit_key(ViewerKey::Tab);
    view.text_input("s");
    assert!(view.panel.has_suggestions());
    assert!(view.click(kernel::Point::new(-50.0, -50.0)).is_none());
    assert!(!view.panel.has_suggestions() && !view.closed());
    assert_eq!(view.panel.scope.text(), "s", "what was typed stays");
}

/// With an option on, every occurrence is found (in the background, coming by `poll`), the
/// index left aside; what isn't a pattern says so instead of results.
#[test]
fn an_option_finds_every_occurrence_or_says_what_s_wrong() {
    let mut view = opened(kernel::ProjectId::new(), "/r", None);
    view.act(ProjectSearchAction::Toggle(crate::search_panel::SearchOption::Case));
    view.text_input("needle");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while view.panel.result(0).is_none() && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::yield_now();
    }
    let hit = view.panel.result(0).expect("the line found");
    assert_eq!((hit.path.as_str(), hit.start_line), ("src/a.php", 1));
    view.panel.input.set_text("");
    view.text_input("Needle");
    std::thread::sleep(std::time::Duration::from_millis(50));
    view.poll();
    assert!(view.panel.result(0).is_none(), "case matters");
    view.act(ProjectSearchAction::Toggle(crate::search_panel::SearchOption::Regex));
    view.panel.input.set_text("");
    view.text_input("(");
    assert!(view.panel.error().is_some() && view.panel.result(0).is_none());
}

/// Opened to replace: every occurrence is found (no option needed), what's typed in the
/// replacement's field doesn't search again, and Replace All asks for one refactoring of them
/// all — the search closing.
#[test]
fn replace_all_asks_for_one_refactoring_of_every_occurrence() {
    let mut view = SearchView::default();
    view.set_data(&SearchData {
        project: kernel::ProjectId::new(),
        queries: Arc::new(Fake("/s")),
        semantic: None,
        selection: None,
        folder: None,
        replace: true,
    });
    assert!(view.panel.replace_open() && !view.panel.in_replace());
    view.text_input("needle");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while view.found.is_empty() && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::yield_now();
    }
    view.edit_key(ViewerKey::Tab);
    assert!(view.panel.in_replace());
    view.text_input("pin");
    assert_eq!(view.panel.input.text(), "needle");
    assert_eq!(view.found.len(), 1, "the replacement typed didn't search again");
    view.act(ProjectSearchAction::ReplaceAll);
    let Some(ViewerAction::ApplyRefactoring(plan)) = view.take_action() else {
        panic!("a refactoring asked for")
    };
    let o = &plan.files[0].occurrences[0];
    assert_eq!((o.original.as_str(), o.replacement.as_str()), ("needle", "pin"));
    assert!(view.closed());
}

/// A mask keeps only its files: in what the index finds and in the files found by name; the
/// search opens again with it in the same project.
#[test]
fn a_mask_keeps_only_its_files() {
    let project = kernel::ProjectId::new();
    let mut view = opened(project, "/m", None);
    view.panel.set_mask("*.twig");
    view.text_input("needle");
    assert!(view.panel.result(0).is_none(), "src/a.php isn't a .twig");
    view.panel.mask.set_text("*.php");
    view.panel.input.set_text("");
    view.text_input("needle");
    assert_eq!(view.panel.result(0).map(|h| h.path.as_str()), Some("src/a.php"));
    let again = opened(project, "/m", None);
    assert_eq!(again.panel.mask.text(), "*.php");
    assert!(again.panel.scope_open(), "its row shows it");
}

/// The folders searched in last come first while the folder's field is empty (newest first,
/// each once, at most six); typing, the folders it goes on to.
#[test]
fn the_recent_folders_are_suggested_first() {
    let mut recent = Vec::new();
    for folder in ["src", "lib", "src", "a", "b", "c", "d", "e"] {
        keep_folder(&mut recent, folder);
    }
    assert_eq!(recent, ["e", "d", "c", "b", "a", "src"]);
    let completed = vec!["lib".to_owned(), "src".to_owned()];
    let both = with_recent("", &["src".to_owned()], completed.clone(), 6);
    assert_eq!(both, ["src", "lib"]);
    assert_eq!(
        with_recent("s", &["lib".to_owned()], completed, 6),
        ["lib", "src"],
        "typed: completed only"
    );
}

/// A result opened from a search in a folder: the folder comes first next time.
#[test]
fn a_folder_searched_in_is_kept() {
    let project = kernel::ProjectId::new();
    let mut view = SearchView::default();
    view.set_data(&SearchData {
        project,
        queries: Arc::new(Fake("/f")),
        semantic: None,
        selection: None,
        folder: Some("src".into()),
        replace: false,
    });
    view.text_input("needle");
    view.act(ProjectSearchAction::Open(0));
    let mut again = opened(project, "/f", None);
    again.edit_key(ViewerKey::Tab);
    assert_eq!(again.panel.suggestion_at(0), Some("src"));
}

/// With a waker (the app), a search runs on a worker: nothing found on the key itself, the
/// results there by a poll once it's done; a newer search replaces the one going.
#[test]
fn a_search_runs_on_a_worker_and_its_results_come_by_a_poll() {
    let mut view = opened(kernel::ProjectId::new(), "/result", None);
    let woken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let told = Arc::clone(&woken);
    view.waker = Some(Arc::new(move || told.store(true, std::sync::atomic::Ordering::SeqCst)));
    view.text_input("nee");
    view.text_input("dle");
    assert!(view.searching.is_some(), "going on its worker");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while view.searching.is_some() && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(woken.load(std::sync::atomic::Ordering::SeqCst), "the window woken");
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::OpenAt { path: PathBuf::from("/result/src/a.php"), line: 3 })
    );
}
