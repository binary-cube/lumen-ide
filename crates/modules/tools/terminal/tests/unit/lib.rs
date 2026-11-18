use super::*;

#[test]
fn registers_its_window_its_shells_and_its_command() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(TerminalModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!((tools[0].id(), tools[0].dock()), (TOOL, Dock::Bottom));
    assert_eq!(tools[0].content(), ToolContent::Views { command: NEW.0, new: Some(NEW.0) });
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    let shell = std::path::PathBuf::from(format!("Local.{EXTENSION}"));
    assert_eq!(viewers[0].kind(&shell), Some(ViewerKind::Replace));
    let ids: Vec<&str> =
        contributions.take::<CommandContribution>().iter().map(|c| c.spec.id.0).collect();
    assert_eq!(
        ids,
        [
            TOGGLE.0,
            NEW.0,
            CLEAR.0,
            COPY_ON_SELECT.0,
            HIGHLIGHT_LINKS.0,
            MOUSE_REPORTING.0,
            CLOSE_ON_EXIT.0,
            SETTINGS.0
        ]
    );
}

#[test]
fn shells_are_numbered_per_project_from_one_again_once_all_are_closed() {
    // Two windows, two projects (by identity: even the same folder opened twice).
    let (a, b) = (Some(kernel::ProjectId::new()), Some(kernel::ProjectId::new()));
    let cwd = Some(std::path::PathBuf::from("/numbering"));
    // No live shells: "Local", then "Local (2)"…
    assert_eq!(next_title(a), "Local");
    let mut open = TerminalView::new();
    ui_components::Viewer::set_data(
        &mut open,
        &TerminalStart { cwd, project: a, ..TerminalStart::default() },
    );
    assert_eq!(next_title(a), "Local (2)");
    // Another project's shells don't count.
    assert_eq!(next_title(b), "Local");
    drop(open);
    assert_eq!(next_title(a), "Local");
}

/// What a fake shell says runs in it.
struct Says(terminal_session::history::Foreground, Option<&'static str>);

impl terminal_session::SessionWatch for Says {
    fn foreground(&self) -> terminal_session::history::Foreground {
        self.0
    }

    fn program(&self) -> Option<String> {
        self.1.map(str::to_owned)
    }

    fn title(&self) -> Option<String> {
        Some("npm run dev".to_owned())
    }
}

/// Before closing: a shell running a program is reported (losing work, stopped by closing
/// its session), one at its prompt — or one that can't tell — isn't.
#[test]
fn a_running_program_is_reported_an_idle_shell_is_not() {
    use terminal_session::history::Foreground;
    let project = Some(kernel::ProjectId::new());
    let shell = |pid, says: Says| view::Shell {
        pid,
        project,
        watch: Some(Arc::new(says) as Arc<dyn terminal_session::SessionWatch>),
    };
    let shells = [
        shell(10, Says(Foreground::Program(std::time::Duration::from_secs(90)), Some("node"))),
        shell(11, Says(Foreground::Shell, None)),
        shell(12, Says(Foreground::Unknown, None)),
        view::Shell { pid: 13, project, watch: None },
    ];
    let running = running_programs(&shells);
    assert_eq!(running.len(), 1);
    let work = &running[0];
    assert_eq!(work.title, "node");
    assert_eq!(work.detail, "npm run dev", "the command line, from the shell's title");
    assert_eq!(work.project, project);
    assert_eq!(work.impact, workbench_model::StopImpact::LosesWork);
    let stop = work.stop.as_ref().unwrap();
    assert_eq!(stop.action, CLOSE_SESSION);
    assert_eq!(stop.target, workbench_model::ActionTarget::Process(10));
    // Without a name, the shell's title names it.
    let unnamed = [shell(14, Says(Foreground::Program(std::time::Duration::from_secs(5)), None))];
    assert_eq!(running_programs(&unnamed)[0].title, "npm run dev");
}
