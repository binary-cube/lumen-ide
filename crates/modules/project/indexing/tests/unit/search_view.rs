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
        // A project with many results (a list that scrolls).
        if self.0 == MANY {
            return (0..MANY_HITS)
                .map(|i| Hit {
                    path: format!("src/f{i}.php"),
                    start_line: 2,
                    end_line: 3,
                    preview: "<?php\n$needle = 1;".into(),
                    score: 1.0,
                })
                .collect();
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
        Arc::new(vec!["src/a.php".into(), "assets/logo.png".into(), "assets/img/cat.png".into()])
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
        let folders: Vec<String> = if self.0 == FOLDERS {
            (0..FOLDER_COUNT).map(|i| format!("dir{i:02}")).collect()
        } else {
            ["lib", "src", "src/sub"].map(String::from).to_vec()
        };
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
    // Said on one line, as words (the regular expression's error is the pattern drawn over
    // a caret, then what's wrong).
    let said = view.panel.error().unwrap();
    assert!(!said.contains('\n') && !said.starts_with("regex"), "{said}");
}

/// A regular expression that isn't one but a file name's pattern (`*.png`): the files
/// named so, by their name (or their path, with a `/`); a pattern of neither: what's wrong.
#[test]
fn a_file_name_s_pattern_lists_the_files_named_so() {
    let mut view = opened(kernel::ProjectId::new(), "/r", None);
    view.act(ProjectSearchAction::Toggle(crate::search_panel::SearchOption::Regex));
    view.text_input("*.png");
    assert_eq!(view.panel.error(), None, "not an error");
    let named: Vec<&str> =
        (0..5).filter_map(|i| view.panel.named(i)).map(|f| f.path.as_str()).collect();
    assert_eq!(named, ["assets/logo.png", "assets/img/cat.png"]);
    view.panel.input.set_text("");
    // (`assets/*.png` is a regular expression too — `s/*` — so its text is searched.)
    view.text_input("*/logo.png");
    let named: Vec<&str> =
        (0..5).filter_map(|i| view.panel.named(i)).map(|f| f.path.as_str()).collect();
    assert_eq!(named, ["assets/logo.png"], "by its path: `*` stays in a folder");
    assert!(looks_like_glob("test_??.rs") && !looks_like_glob("(a|b)*") && !looks_like_glob("abc"));
    assert_eq!(
        readable_error(
            "regex parse error:\n    *.png\n    ^\nerror: repetition operator missing expression"
        ),
        "Repetition operator missing expression"
    );
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

/// Without any option, a file name's pattern lists its files too (as one word: with a
/// space it's searched as text).
#[test]
fn a_file_name_s_pattern_needs_no_option() {
    let mut view = opened(kernel::ProjectId::new(), "/r", None);
    view.text_input("*.png");
    let named: Vec<&str> =
        (0..5).filter_map(|i| view.panel.named(i)).map(|f| f.path.as_str()).collect();
    assert_eq!(named, ["assets/logo.png", "assets/img/cat.png"]);
    assert!(view.panel.result(0).is_none(), "no text searched");
}

/// A recent search forgotten (its ×, or ⌫ on it), then every one ("Clear all"): kept so
/// for the project (the search opened again shows the same).
#[test]
fn recent_searches_are_forgotten_one_or_all() {
    let project = kernel::ProjectId::new();
    let mut view = opened(project, "/result", None);
    for query in ["alpha", "beta", "gamma"] {
        view.panel.input.set_text(query);
        view.remember();
    }
    view.panel.input.set_text("");
    assert_eq!(view.panel.recent_searches(), ["gamma", "beta", "alpha"]);
    view.act(ProjectSearchAction::ForgetRecent(1));
    assert_eq!(view.panel.recent_searches(), ["gamma", "alpha"]);
    assert!(view.edit_key(ViewerKey::Backspace), "on the first one: forgotten");
    assert_eq!(view.panel.recent_searches(), ["alpha"]);
    assert_eq!(opened(project, "/result", None).panel.recent_searches(), ["alpha"]);
    view.act(ProjectSearchAction::ClearRecent);
    assert!(opened(project, "/result", None).panel.recent_searches().is_empty());
}

/// The root of the project whose search finds many files.
const MANY: &str = "/many";
/// The files it finds.
const MANY_HITS: usize = 40;

/// The results' scrollbar: its thumb pressed and dragged down scrolls the rows; let go, the
/// moves no longer reach it.
#[test]
fn the_results_scroll_by_their_thumb() {
    let mut view = opened(kernel::ProjectId::new(), MANY, None);
    view.text_input("needle");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while view.searching.is_some() && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let bounds = kernel::Rect::new(0.0, 0.0, 1200.0, 800.0);
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds,
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let on_bar = (0..800)
        .flat_map(|y| (0..1200).rev().map(move |x| Point::new(x as f32, y as f32)))
        .find(|&p| view.panel.on_bar(p))
        .expect("a scrollbar");
    view.hover(on_bar);
    assert!(view.click(on_bar).is_none(), "the thumb takes the press");
    let before = view.panel.first_row();
    assert!(view.drag(Point::new(on_bar.x, on_bar.y + 120.0)), "the rows follow");
    assert!(view.panel.first_row() > before);
    view.release();
    assert!(!view.drag(Point::new(on_bar.x, on_bar.y + 200.0)), "let go: not held");
}

/// The view's results searched and drawn (the many files' project).
fn painted_with_many() -> SearchView {
    let mut view = opened(kernel::ProjectId::new(), MANY, None);
    view.text_input("needle");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while view.searching.is_some() && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 800.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    view
}

/// The panel is the view's one control, with the keyboard from the start: what's typed
/// and the keys reach its fields through the controls, and Tab stays in it (it opens the
/// folder's row, then goes on between its fields).
#[test]
fn the_keyboard_is_the_panel_s_from_the_start() {
    let mut view = opened(kernel::ProjectId::new(), "/keys", None);
    assert_eq!(view.controls.focused(), Some(PANEL));
    view.text_input("ab");
    assert!(view.edit_key_with(ViewerKey::Left, KeyReach::default()));
    view.text_input("x");
    assert_eq!(view.panel.input.text(), "axb", "the caret moved by the key");
    assert!(view.edit_key(ViewerKey::Tab));
    assert!(view.panel.in_scope(), "Tab opened the folder's row");
    assert!(view.edit_key_with(ViewerKey::Tab, KeyReach { word: false, extend: true }));
    assert!(view.panel.in_mask(), "⇧Tab goes on too, as before");
    assert_eq!(view.controls.focused(), Some(PANEL), "the focus stays in the panel");
    assert!(view.command("select_all"));
}

/// The wheel moves the results by rows: under the mouse, or with the mouse not moved yet.
#[test]
fn the_results_scroll_by_the_wheel() {
    let mut view = painted_with_many();
    let before = view.panel.first_row();
    assert!(view.scroll(0.0, 60.0), "with no hover yet, the wheel is the panel's");
    assert!(view.panel.first_row() > before);
    let moved = view.panel.first_row();
    view.hover(Point::new(600.0, 400.0));
    assert!(view.scroll(0.0, -60.0));
    assert!(view.panel.first_row() < moved);
    assert!(!view.scroll(0.0, 1.0), "less than a row: nothing");
}

/// The cursor and the tooltip are answered where the mouse is, as the panel said there.
#[test]
fn the_cursor_and_the_tooltip_follow_the_mouse() {
    let mut view = painted_with_many();
    let button = view.panel.scope_button();
    let on = button.center();
    assert_eq!(view.cursor_at(on), None, "not hovered yet");
    view.hover(on);
    assert_eq!(view.cursor_at(on), Some(ViewerCursor::Pointer));
    assert!(view.tooltip_at(on).is_some(), "the folder's button says what it does");
    let away = Point::new(-10.0, -10.0);
    view.hover(away);
    assert_eq!(view.cursor_at(away), None);
    assert!(view.tooltip_at(away).is_none());
}

/// Paints the view in a window-sized area.
fn repainted(view: &mut SearchView) {
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 800.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// The search is the app's search field: a line break typed or pasted is a space; its
/// replacement's button, options and folder's button sit in the room it keeps on its
/// right, its × left of them (a hand over it, the text's cursor over the text); × pressed
/// empties it.
#[test]
fn the_search_is_the_shared_field_with_its_buttons_inside() {
    let mut view = painted_with_many();
    view.panel.input.set_text("");
    assert!(view.paste("ne\nedle"));
    assert_eq!(view.panel.input.text(), "ne edle", "one line: a space");
    repainted(&mut view);
    let field = view.panel.input.area();
    let clear = view.panel.input.clear_area().expect("× shown with text");
    let (replace, folder) = (view.panel.replace_button(), view.panel.scope_button());
    assert!(field.contains(replace.center()) && field.contains(folder.center()));
    assert!(clear.max_x() <= replace.min_x(), "× left of the buttons");
    let option = Point::new((replace.max_x() + folder.min_x()) / 2.0, folder.center().y);
    assert!(view.panel.option_at(option).is_some(), "the options between them");
    view.hover(clear.center());
    assert_eq!(view.cursor_at(clear.center()), Some(ViewerCursor::Pointer));
    let on_text = Point::new(field.min_x() + 40.0, field.center().y);
    view.hover(on_text);
    assert_eq!(view.cursor_at(on_text), Some(ViewerCursor::Text));
    view.press(clear.center(), 1);
    assert_eq!(view.panel.input.text(), "", "× empties it");
    assert!(view.panel.showing_recent(), "searched again: the recent searches");
}

/// An option turned on shows on its button (the shared one, its back in the accent), and
/// only there: off, none of the field's buttons has one.
#[test]
fn an_option_turned_on_is_lit_on_its_button() {
    let palette = ui_components::Palette::dark();
    let accent = palette.accent;
    let backs = |view: &mut SearchView| {
        let mut text = text_layout::TextSystem::new();
        let icons = icons::Icons::builtin();
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 800.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
        let field = view.panel.input.area();
        scene
            .layers()
            .iter()
            .flat_map(|l| &l.quads)
            .filter(|q| {
                let c = q.color;
                (c.r, c.g, c.b) == (accent.r, accent.g, accent.b)
                    && c.a > 0.0
                    && c.a < 1.0
                    && field.contains(q.bounds.center())
            })
            .map(|q| q.bounds)
            .collect::<Vec<_>>()
    };
    let mut view = opened(kernel::ProjectId::new(), "/r", None);
    assert!(backs(&mut view).is_empty(), "every option off");
    view.act(ProjectSearchAction::Toggle(crate::search_panel::SearchOption::Case));
    let on = backs(&mut view);
    let (replace, folder) = (view.panel.replace_button(), view.panel.scope_button());
    assert_eq!(on.len(), 1, "the one turned on");
    assert!(on[0].min_x() >= replace.max_x() && on[0].max_x() <= folder.min_x());
}

/// The root of the project with many folders (more suggested than a short window shows).
const FOLDERS: &str = "/folders";
/// Its folders (`dir00`, `dir01`…), all at its top.
const FOLDER_COUNT: usize = 12;

/// Paints the view in an area `width` × `height`.
fn painted_at(view: &mut SearchView, width: f32, height: f32) {
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, width, height),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// The folders suggested are the shared list opened under the folder's field, over the
/// results: the panel (its header) as tall with them as without.
#[test]
fn the_folders_suggested_open_as_a_list_over_the_results() {
    let mut view = opened(kernel::ProjectId::new(), FOLDERS, None);
    view.edit_key(ViewerKey::Tab);
    assert!(view.panel.has_suggestions());
    painted_at(&mut view, 1200.0, 800.0);
    let with = view.panel.panel_area();
    let (area, _) = view.panel.suggestions_area().expect("the list drawn");
    let field = view.panel.scope_field();
    assert!(area.min_y() >= field.max_y(), "under the folder's field");
    assert!(area.max_y() > field.max_y() + 100.0, "over the results, past the header");
    view.edit_key(ViewerKey::Escape);
    assert!(!view.panel.has_suggestions() && !view.closed());
    painted_at(&mut view, 1200.0, 800.0);
    let without = view.panel.panel_area();
    assert_eq!(without.size, with.size, "the header keeps its height");
    // A click on a row takes its folder.
    view.edit_key(ViewerKey::Tab);
    view.edit_key(ViewerKey::Tab);
    view.edit_key(ViewerKey::Tab);
    assert!(view.panel.in_scope());
    view.text_input("dir0");
    painted_at(&mut view, 1200.0, 800.0);
    let (_, rows) = view.panel.suggestions_area().expect("the list drawn");
    let second = Point::new(rows.center().x, rows.min_y() + 28.0 * 1.5);
    view.hover(second);
    assert_eq!(view.cursor_at(second), Some(ViewerCursor::Pointer), "a hand on its rows");
    assert!(view.click(second).is_none());
    assert_eq!(view.panel.scope.text(), "dir01/");
}

/// In a short window the list fits the room under its field: fewer rows, scrolled by the
/// wheel and by its thumb dragged (the press choosing nothing).
#[test]
fn the_folders_suggested_fit_a_short_window_and_scroll() {
    let mut view = opened(kernel::ProjectId::new(), FOLDERS, None);
    view.edit_key(ViewerKey::Tab);
    painted_at(&mut view, 1200.0, 340.0);
    let (area, rows) = view.panel.suggestions_area().expect("the list drawn");
    assert!(area.max_y() <= 340.0, "within the window");
    assert!(rows.size.height < FOLDER_COUNT as f32 * 28.0, "fewer rows than suggested");
    let thumb = Point::new(rows.max_x() - 4.0, rows.min_y() + 6.0);
    view.hover(thumb);
    assert!(view.click(thumb).is_none());
    assert!(view.panel.has_suggestions(), "the bar took the press: nothing chosen");
    assert_eq!(view.panel.scope.text(), "");
    assert!(view.drag(Point::new(thumb.x, thumb.y + 120.0)), "the rows follow the thumb");
    view.release();
    assert!(!view.drag(Point::new(thumb.x, thumb.y + 160.0)), "let go: not held");
    view.hover(rows.center());
    assert!(view.scroll(0.0, -200.0), "the wheel over the list scrolls it");
    assert_eq!(view.panel.first_row(), 0, "not the results under it");
}

/// The keys choose a folder: ↓ ↑ move through the list, ↵ takes the lit one (and goes into
/// it), esc closes the list before the search.
#[test]
fn the_keys_choose_a_folder_suggested() {
    let mut view = opened(kernel::ProjectId::new(), FOLDERS, None);
    view.edit_key(ViewerKey::Tab);
    painted_at(&mut view, 1200.0, 340.0);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Up);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.panel.scope.text(), "dir02/");
    assert!(!view.panel.has_suggestions(), "no folders in it");
    // The caret after it: what's typed goes on from there.
    for _ in 0..3 {
        view.edit_key(ViewerKey::Backspace);
    }
    view.text_input("1");
    assert_eq!(view.panel.scope.text(), "dir1");
    assert!(view.panel.has_suggestions(), "typing filters them");
    assert_eq!(view.panel.suggestion_at(0), Some("dir10"));
    view.edit_key(ViewerKey::Escape);
    assert!(!view.panel.has_suggestions() && !view.closed(), "esc closed only the list");
}

/// Every folder is offered, more than fit: the list scrolls to the last one (by its thumb,
/// by the keys), and a path typed lists that folder's own.
#[test]
fn every_folder_is_offered_and_the_list_scrolls_to_the_last() {
    let mut view = opened(kernel::ProjectId::new(), FOLDERS, None);
    view.edit_key(ViewerKey::Tab);
    let last = format!("dir{:02}", FOLDER_COUNT - 1);
    assert_eq!(view.panel.suggestion_at(FOLDER_COUNT - 1), Some(last.as_str()), "all of them");
    assert_eq!(view.panel.suggestion_at(0), Some("dir00"), "by name");
    painted_at(&mut view, 1200.0, 800.0);
    let (_, rows) = view.panel.suggestions_area().expect("the list drawn");
    let thumb = Point::new(rows.max_x() - 4.0, rows.min_y() + 6.0);
    view.hover(thumb);
    assert!(view.click(thumb).is_none());
    assert!(view.drag(Point::new(thumb.x, rows.max_y() + 400.0)), "dragged to the end");
    view.release();
    painted_at(&mut view, 1200.0, 800.0);
    let bottom = Point::new(rows.center().x, rows.max_y() - 14.0);
    view.hover(bottom);
    assert!(view.click(bottom).is_none());
    assert_eq!(view.panel.scope.text(), format!("{last}/"), "the last row shown and taken");
    // ↑ from the first goes round to the last.
    let mut view = opened(kernel::ProjectId::new(), FOLDERS, None);
    view.edit_key(ViewerKey::Tab);
    view.edit_key(ViewerKey::Up);
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.panel.scope.text(), format!("{last}/"));
    // A path typed: that folder's own.
    let mut view = opened(kernel::ProjectId::new(), "/sub", None);
    view.edit_key(ViewerKey::Tab);
    view.text_input("src/");
    assert_eq!(view.panel.suggestion_at(0), Some("src/sub"));
}
