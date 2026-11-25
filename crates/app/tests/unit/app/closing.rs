use super::*;
use crate::app::tests::{app, app_with};

#[test]
fn line_and_column_as_typed() {
    assert_eq!(parse_line_column("12"), Some((11, 0)));
    assert_eq!(parse_line_column(" 12:5 "), Some((11, 4)));
    assert_eq!(parse_line_column("12,5"), Some((11, 4)));
    assert_eq!(parse_line_column("0"), Some((0, 0)));
    assert_eq!(parse_line_column("x"), None);
    assert_eq!(parse_line_column(""), None);
}

/// The open dialog answered with its button `id`, as a click does.
fn answer(app: &mut EditorApp, id: &'static str) {
    assert!(app.workbench.answer_dialog(Some(id)));
    let answer = app.workbench.take_dialog_answer().unwrap();
    app.on_dialog(answer);
}

/// A scratch project with two files.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("closing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "<?php\necho 1;\n").unwrap();
    std::fs::write(dir.join("b.php"), "<?php\necho 2;\n").unwrap();
    crate::workbench::canonical(&dir)
}

/// With nothing unsaved, Close Project goes straight to the start screen, the window kept.
#[test]
fn closing_a_saved_project_shows_the_start_screen() {
    let dir = project("saved");
    let mut app = app();
    assert!(!app.close_project(), "nothing to close without a project");
    assert!(app.open_project(&dir));
    app.workbench.open_file(&dir.join("a.php")).unwrap();
    assert!(app.close_project());
    assert!(!app.workbench.dialog_open());
    assert!(app.workbench.project_root().is_none());
    assert_eq!(app.workbench.tab_count(), 0);
    assert!(!app.workbench.has_content());
    assert!(app.pending.exit_request.is_none(), "the window stays");
    assert_eq!(app.workbench.title(), kernel::brand::NAME);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An unsaved file asks first, as closing the window does; Cancel keeps the project, its
/// tabs and the change; Discard then closes it.
#[test]
fn unsaved_files_are_asked_about_and_cancel_keeps_everything() {
    let dir = project("unsaved");
    let mut app = app();
    assert!(app.open_project(&dir));
    app.workbench.open_file(&dir.join("a.php")).unwrap();
    app.workbench.open_file(&dir.join("b.php")).unwrap();
    let edited = app.with_text(|wb, text| wb.edit(text, |d| edit::insert_text(d, "x")));
    assert_eq!(edited, Some(true));
    assert!(app.close_project());
    assert!(app.workbench.dialog_open(), "the unsaved file is asked about");
    assert!(matches!(app.pending.asking, Some(Ask::Save(_))));
    answer(&mut app, "cancel");
    assert_eq!(app.workbench.project_root(), Some(dir.clone()));
    assert_eq!(app.workbench.tab_count(), 2);
    assert!((0..2).any(|i| app.workbench.tab_dirty(i)), "the change is kept");
    assert!(app.pending.closing.is_none());

    assert!(app.close_project());
    answer(&mut app, "discard");
    assert!(app.workbench.project_root().is_none());
    assert_eq!(app.workbench.tab_count(), 0);
    assert_eq!(std::fs::read_to_string(dir.join("b.php")).unwrap(), "<?php\necho 2;\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Work still running, as a module reports it: what's listed, what was stopped.
#[derive(Debug, Default)]
struct Busy {
    work: std::sync::Mutex<Vec<workbench_model::RunningWork>>,
    stopped: std::sync::Mutex<Vec<String>>,
}

impl Busy {
    fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Work `title` for `project` (`None`: the app's), stoppable, losing work if cut.
    fn runs(&self, title: &str, project: Option<kernel::ProjectId>) {
        use workbench_model::running::{StopVia, WorkKind};
        let mut work = workbench_model::RunningWork::new(
            WorkKind::Program,
            title,
            "test.busy",
            project,
            workbench_model::StopImpact::LosesWork,
        );
        work.stop = Some(StopVia {
            part: workbench_model::PartRef {
                owner: "test.busy".into(),
                name: title.into(),
                project,
            },
            action: "stop",
            target: workbench_model::ActionTarget::Part,
        });
        self.work.lock().unwrap().push(work);
    }

    fn ends(&self, title: &str) {
        self.work.lock().unwrap().retain(|w| w.title != title);
    }

    fn stopped(&self) -> Vec<String> {
        self.stopped.lock().unwrap().clone()
    }
}

/// The reporter of a [`Busy`].
#[derive(Debug)]
struct BusyReporter(Arc<Busy>);

impl workbench_model::ResourceReporter for BusyReporter {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        Vec::new()
    }

    fn running(&self) -> Vec<workbench_model::RunningWork> {
        self.0.work.lock().unwrap().clone()
    }

    fn act(
        &self,
        part: &workbench_model::PartRef,
        action: &str,
        _target: workbench_model::ActionTarget,
    ) -> workbench_model::ActOutcome {
        assert_eq!(action, "stop");
        self.0.stopped.lock().unwrap().push(part.name.clone());
        self.0.ends(&part.name);
        workbench_model::ActOutcome::Done
    }
}

fn busy_app(busy: &Arc<Busy>, shared: Option<Rc<Shared>>) -> EditorApp {
    app_with(vec![Arc::new(BusyReporter(Arc::clone(busy)))], shared)
}

fn project_id(app: &EditorApp) -> kernel::ProjectId {
    app.workbench.project_identity().unwrap().0
}

/// "Stop and close": what runs is stopped through its reporter, then the closing goes on
/// (Close Project: the start screen); asked before anything closes.
#[test]
fn stop_and_close_stops_the_work_then_closes() {
    let dir = project("running-stop");
    let busy = Busy::shared();
    let mut app = busy_app(&busy, None);
    assert!(app.open_project(&dir));
    app.workbench.open_file(&dir.join("a.php")).unwrap();
    busy.runs("upload", Some(project_id(&app)));
    assert!(app.close_project());
    assert!(app.workbench.running_prompt_open(), "asked in the module's modal");
    assert_eq!(app.workbench.project_root(), Some(dir.clone()), "nothing closed yet");
    assert_eq!(app.workbench.tab_count(), 1);
    // The window's button meanwhile: the question stands.
    assert_eq!(app.close_window(), Control::Continue);
    app.on_running_answer(workbench_model::CloseAnswer::StopAndClose);
    assert_eq!(busy.stopped(), ["upload"]);
    assert!(!app.workbench.running_prompt_open());
    assert!(app.workbench.project_root().is_none(), "closed after");
    assert!(app.pending.running.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// "Wait": the window stays, the question follows the work; once it's done the window
/// closes by itself.
#[test]
fn wait_closes_the_window_once_the_work_is_done() {
    let dir = project("running-wait");
    let busy = Busy::shared();
    let mut app = busy_app(&busy, None);
    assert!(app.open_project(&dir));
    busy.runs("push", Some(project_id(&app)));
    busy.runs("query", Some(project_id(&app)));
    assert_eq!(app.close_window(), Control::Continue);
    app.on_running_answer(workbench_model::CloseAnswer::Wait);
    assert!(app.workbench.running_prompt_open());
    let later = |s| Instant::now() + std::time::Duration::from_secs(s);
    busy.ends("push");
    app.follow_running(later(1));
    assert!(app.pending.exit_request.is_none(), "one still runs");
    assert!(app.workbench.running_prompt_open());
    busy.ends("query");
    app.follow_running(later(2));
    assert_eq!(app.pending.exit_request.take(), Some(Control::Close));
    assert!(busy.stopped().is_empty(), "nothing was stopped");
    assert!(!app.workbench.running_prompt_open());
    let _ = std::fs::remove_dir_all(&dir);
}

/// "Cancel" — or the question closed with × — leaves everything as it was.
#[test]
fn cancel_or_closing_the_question_keeps_everything() {
    let dir = project("running-cancel");
    let busy = Busy::shared();
    let mut app = busy_app(&busy, None);
    assert!(app.open_project(&dir));
    app.workbench.open_file(&dir.join("a.php")).unwrap();
    busy.runs("program", Some(project_id(&app)));
    assert_eq!(app.quit(), Control::Continue);
    app.on_running_answer(workbench_model::CloseAnswer::Cancel);
    assert!(app.pending.running.is_none() && app.pending.exit_request.is_none());
    assert!(!app.workbench.running_prompt_open());
    assert_eq!(app.workbench.tab_count(), 1);
    assert_eq!(app.workbench.project_root(), Some(dir.clone()));
    // ×: the question gone without an answer is a Cancel.
    assert_eq!(app.quit(), Control::Continue);
    app.workbench.close_running_prompt();
    app.follow_running(Instant::now());
    assert!(app.pending.running.is_none() && app.pending.exit_request.is_none());
    assert!(busy.stopped().is_empty());
    // Nothing running: no question, the closing goes straight on.
    busy.ends("program");
    assert_eq!(app.quit(), Control::Exit);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Close Project asks only about its own work: not another project's, not the app's (the
/// app goes on); closing a window that isn't the last doesn't ask about the app's work
/// either, the last one does.
#[test]
fn closing_a_project_asks_only_about_its_own_work() {
    let (a, b) = (project("running-scope-a"), project("running-scope-b"));
    let busy = Busy::shared();
    let mut first = busy_app(&busy, None);
    let mut second = busy_app(&busy, Some(Rc::clone(&first.shared)));
    assert!(first.open_project(&a) && second.open_project(&b));
    busy.runs("mine", Some(project_id(&first)));
    busy.runs("theirs", Some(project_id(&second)));
    busy.runs("app's", None);
    let ask = |app: &EditorApp| app.pending.running.as_ref().map(|r| r.scope);
    assert!(first.close_project());
    let scope = ask(&first).unwrap();
    let asked: Vec<String> =
        first.workbench.running_work(&scope).into_iter().map(|w| w.title).collect();
    assert_eq!(asked, ["mine"]);
    first.on_running_answer(workbench_model::CloseAnswer::StopAndClose);
    assert_eq!(busy.stopped(), ["mine"], "only its own");
    // Two windows: the second's isn't the last, the app's work isn't asked about.
    assert_eq!(second.close_window(), Control::Continue);
    let asked: Vec<String> = second
        .workbench
        .running_work(&ask(&second).unwrap())
        .into_iter()
        .map(|w| w.title)
        .collect();
    assert_eq!(asked, ["theirs"]);
    second.on_running_answer(workbench_model::CloseAnswer::Cancel);
    // The last window: the app's too.
    first.shared.windows.set(1);
    busy.ends("theirs");
    let bare = first.close_window();
    assert_eq!(bare, Control::Continue, "the app's work is asked about");
    let asked: Vec<String> =
        first.workbench.running_work(&ask(&first).unwrap()).into_iter().map(|w| w.title).collect();
    assert_eq!(asked, ["app's"]);
    let _ = (std::fs::remove_dir_all(&a), std::fs::remove_dir_all(&b));
}

/// Quitting with two windows: the one quitting asks once about everything — both projects'
/// work and the app's, by their names — then the other window only about its files.
#[test]
fn quitting_with_two_windows_asks_once_about_everything() {
    let (a, b) = (project("running-quit-a"), project("running-quit-b"));
    let busy = Busy::shared();
    let mut first = busy_app(&busy, None);
    let mut second = busy_app(&busy, Some(Rc::clone(&first.shared)));
    assert!(first.open_project(&a) && second.open_project(&b));
    *first.shared.project_names.borrow_mut() =
        [&first, &second].iter().filter_map(|app| app.project_identity()).collect();
    busy.runs("upload", Some(project_id(&first)));
    busy.runs("program", Some(project_id(&second)));
    busy.runs("download", None);
    assert_eq!(first.quit(), Control::Continue);
    let scope = first.pending.running.as_ref().unwrap().scope;
    let items = first.workbench.running_work(&scope);
    assert_eq!(items.len(), 3);
    let others = first.shared.project_names.borrow().clone();
    let prompt = first.workbench.running_prompt(
        workbench_model::running::Closing::App,
        items,
        false,
        &others,
    );
    assert_eq!(prompt.groups.len(), 3, "by project, the app's last");
    assert_eq!(prompt.groups[2].project, None);
    let names: Vec<&str> = prompt.projects.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names.len(), 2, "both windows' projects named: {names:?}");
    first.on_running_answer(workbench_model::CloseAnswer::StopAndClose);
    let mut stopped = busy.stopped();
    stopped.sort();
    assert_eq!(stopped, ["download", "program", "upload"]);
    assert_eq!(first.pending.exit_request.take(), Some(Control::Exit));
    // `AppWindows` then asks the other window about its files only (none unsaved).
    assert!(second.prepare_quit());
    assert!(second.pending.running.is_none());
    let _ = (std::fs::remove_dir_all(&a), std::fs::remove_dir_all(&b));
}

/// A module restarting its work for a project (a remote project reconnecting) asks through
/// its host about its own work only, in the same question; "Stop" stops that work and runs
/// the restart, nothing closes; with nothing of its own running, the restart runs at once;
/// "Cancel" never runs it.
#[test]
fn a_module_s_restart_asks_about_its_own_work_then_runs() {
    use workbench_model::WorkbenchHost;
    let dir = project("running-restart");
    let busy = Busy::shared();
    let mut app = busy_app(&busy, None);
    assert!(app.open_project(&dir));
    let id = project_id(&app);
    busy.runs("upload", Some(id));
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let restart = |ran: &Arc<std::sync::atomic::AtomicUsize>| -> workbench_model::OnYes {
        let ran = Arc::clone(ran);
        Box::new(move |_| {
            ran.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
    };
    let count =
        |ran: &Arc<std::sync::atomic::AtomicUsize>| ran.load(std::sync::atomic::Ordering::Relaxed);
    // Another module's work: not asked about, it runs at once.
    assert!(app.workbench.ask_running(id, "other.module", restart(&ran)));
    let request = app.workbench.take_running_request().unwrap();
    app.ask_running_request(request);
    assert_eq!(count(&ran), 1);
    assert!(app.pending.running.is_none());
    // Its own: asked first (the restart's question), Cancel never runs it.
    app.workbench.ask_running(id, "test.busy", restart(&ran));
    let request = app.workbench.take_running_request().unwrap();
    app.ask_running_request(request);
    assert!(app.workbench.running_prompt_open());
    assert_eq!(
        app.pending.running.as_ref().map(|r| r.closing),
        Some(workbench_model::running::Closing::Restart)
    );
    app.on_running_answer(workbench_model::CloseAnswer::Cancel);
    assert_eq!(count(&ran), 1);
    // Stop: stopped, then it runs — the project and its tabs stay.
    app.workbench.ask_running(id, "test.busy", restart(&ran));
    let request = app.workbench.take_running_request().unwrap();
    app.ask_running_request(request);
    app.on_running_answer(workbench_model::CloseAnswer::StopAndClose);
    assert_eq!(busy.stopped(), ["upload"]);
    assert_eq!(count(&ran), 2);
    assert_eq!(app.workbench.project_root(), Some(dir.clone()), "nothing closed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project open in another window: opening it asks — go to its window, or open it again
/// (the user decides); Cancel leaves everything as it is.
#[test]
fn a_project_open_elsewhere_asks_where_to_go() {
    let dir = project("open-elsewhere");
    let mut app = app();
    let other = platform_api::WindowId(424_242);
    app.shared.open_projects.borrow_mut().push((other, dir.clone()));
    assert!(app.open_project(&dir));
    assert!(app.workbench.dialog_open());
    assert!(
        matches!(&app.pending.asking, Some(Ask::AlreadyOpen(p, w)) if *p == dir && *w == other)
    );
    answer(&mut app, "cancel");
    assert!(app.workbench.project_root().is_none(), "nothing opened");
    // Opened again here (this window holds nothing): it opens.
    assert!(app.open_project(&dir));
    answer(&mut app, "again");
    assert_eq!(app.workbench.project_root(), Some(dir.clone()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An update asked for (the updates' module) quits as Quit does: with nothing to ask, the
/// app exits holding it for its windows to hand over; an unsaved file is asked about first,
/// and Cancel keeps everything (nothing exits).
#[test]
fn an_update_quits_as_quit_does() {
    let staged = PathBuf::from("/updates/2.0.0");
    let mut app = app();
    app.viewer_request(ui_workbench::AppRequest::RestartToUpdate(staged.clone()));
    assert_eq!(app.pending.exit_request, Some(Control::Exit));
    assert_eq!(app.take_update(), Some(staged.clone()));
    assert_eq!(app.take_update(), None, "taken once");

    let dir = project("update");
    let mut app = self::app();
    assert!(app.open_project(&dir));
    app.workbench.open_file(&dir.join("a.php")).unwrap();
    let edited = app.with_text(|wb, text| wb.edit(text, |d| edit::insert_text(d, "x")));
    assert_eq!(edited, Some(true));
    app.viewer_request(ui_workbench::AppRequest::RestartToUpdate(staged));
    assert!(app.workbench.dialog_open(), "the unsaved file is asked about");
    answer(&mut app, "cancel");
    assert!(app.pending.exit_request.is_none(), "Cancel: the app stays");
    assert_eq!(
        app.take_update(),
        None,
        "Cancel: the update forgotten (a later Quit installs nothing)"
    );
    assert_eq!(app.workbench.tab_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window's own close button (drawn by the app) closes as the system's does: the work
/// still running asked about first, nothing closed meanwhile; with nothing running, the
/// window goes after the event.
#[test]
fn the_bar_s_close_button_asks_about_the_work_still_running() {
    let dir = project("running-button");
    let busy = Busy::shared();
    let mut app = busy_app(&busy, None);
    assert!(app.open_project(&dir));
    busy.runs("upload", Some(project_id(&app)));
    app.mouse_result(crate::workbench::MouseResult::CloseWindow);
    assert!(app.workbench.running_prompt_open(), "asked first");
    assert!(app.pending.exit_request.is_none(), "the window stays meanwhile");
    app.on_running_answer(workbench_model::CloseAnswer::Cancel);
    busy.ends("upload");
    app.mouse_result(crate::workbench::MouseResult::CloseWindow);
    assert_eq!(app.pending.exit_request.take(), Some(Control::Close), "nothing runs: it goes");
    let _ = std::fs::remove_dir_all(&dir);
}
