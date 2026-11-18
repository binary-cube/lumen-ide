//! The bar: its tab's title, icon, color and breadcrumb from the source; its buttons' commands
//! and menus; the statement Run would run highlighted while it's hovered; its modes kept
//! with the console; a tab closing with a transaction open asking first; what a run did.

use super::*;
use crate::fake::{FakeAccess, Manual, Project, consoles};
use database::console::ConsoleAccess;
use kernel::Rect;
use ui_components::editor_toolbar::EditorToolbar;

struct Bar {
    _project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<Consoles>,
    bar: ConsoleBar,
}

fn bar(name: &str, text: &str) -> Bar {
    bar_with(name, text, |_| {})
}

fn bar_with(name: &str, text: &str, change: impl FnOnce(&mut FakeAccess)) -> Bar {
    let project = Project::new(name);
    let manual = Manual::new();
    let mut fake = FakeAccess::new(&project);
    change(&mut fake);
    let access = Arc::new(fake);
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, text).unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let bar = ConsoleBar::new(Arc::clone(&consoles), spec, runner);
    Bar { _project: project, manual, access, consoles, bar }
}

/// Drawn once, `width` wide: its controls' places known.
fn paint(bar: &mut ConsoleBar, width: f32) {
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    bar.paint(ToolbarPaint {
        bounds: Rect::new(0.0, 0.0, width, HEIGHT),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
    });
}

fn center(bar: &ConsoleBar, tool: Tool) -> Point {
    let (r, _) = bar.hits.iter().find(|(_, t)| *t == tool).unwrap_or_else(|| panic!("{tool:?}"));
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

#[test]
fn its_tab_is_the_console_of_its_source() {
    let b = bar("title", "");
    assert_eq!(b.bar.title().as_deref(), Some("console [shop]"));
    assert_eq!(b.bar.icon(), Some("database"));
    assert!(b.bar.accent(&Palette::dark()).is_some(), "the source's color");
    assert_eq!(
        b.bar.crumbs(),
        [i18n::t("console.crumb_consoles"), "shop".into(), "console".into()]
    );
    assert_eq!(b.bar.height(), HEIGHT);
}

/// ▶ runs the module's command; ■ is off while nothing runs; ▾ and the pills open their
/// menus under them.
#[test]
fn its_buttons_run_commands_and_open_menus() {
    let mut b = bar("buttons", "SELECT 1;");
    paint(&mut b.bar, 1100.0);
    let run = b.bar.pointer_down(center(&b.bar, Tool::Run));
    assert_eq!(run, Some(ToolbarAction::View(ViewerAction::RunCommand(crate::RUN.0.into()))));
    assert!(!b.bar.hits.iter().any(|(_, t)| *t == Tool::Stop), "nothing runs: Stop is off");
    let Some(ToolbarAction::Menu { entries, .. }) =
        b.bar.pointer_down(center(&b.bar, Tool::RunMenu))
    else {
        panic!("Run's menu");
    };
    let ids: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        ["run", "run-script", "explain", "explain-analyze"],
        "SQLite explains, and analyzes through its measured profile"
    );
    assert!(matches!(
        b.bar.pointer_down(center(&b.bar, Tool::Tx)),
        Some(ToolbarAction::Menu { .. })
    ));
    assert!(matches!(
        b.bar.pointer_down(center(&b.bar, Tool::Mode)),
        Some(ToolbarAction::Menu { .. })
    ));
    // `(p)`: the values of its parameters.
    let params = b.bar.pointer_down(center(&b.bar, Tool::Params));
    assert_eq!(params, Some(ToolbarAction::View(ViewerAction::RunCommand(crate::PARAMS.0.into()))));
    let services = b.bar.pointer_down(center(&b.bar, Tool::Services));
    assert_eq!(
        services,
        Some(ToolbarAction::View(ViewerAction::RunCommand(
            workbench_model::TOGGLE_SERVICES.0.into()
        )))
    );
    // Every control says what it is.
    for (rect, tool) in b.bar.hits.clone() {
        let p = Point::new(rect.min_x() + 1.0, rect.min_y() + 1.0);
        assert!(b.bar.tooltip_at(p).is_some(), "{tool:?}");
    }
    let tip = b.bar.tooltip_at(center(&b.bar, Tool::Run)).unwrap();
    assert_eq!(tip.label, i18n::t("console.tip_run"));
}

/// The modes chosen in its menus are the console's (kept with it).
#[test]
fn its_modes_are_kept_with_the_console() {
    let mut b = bar("modes", "");
    assert!(b.bar.command("tx:manual"));
    assert!(b.bar.command("mode:script"));
    assert!(b.bar.command("isolation:immediate"));
    assert!(b.bar.command("schema:main"));
    let spec = b.consoles.spec(b.bar.console()).unwrap();
    assert_eq!(spec.tx, TxMode::Manual);
    assert_eq!(spec.mode, RunMode::Script);
    assert_eq!(spec.isolation, Some(Isolation::Immediate));
    assert_eq!(spec.schema.as_deref(), Some("main"));
    assert_eq!(b.bar.place_label(), "main");
    assert!(!b.bar.command("unknown"));
}

/// Resting on ▶: the statement it would run highlighted (the editor followed meanwhile).
#[test]
fn run_hovered_highlights_its_statement() {
    let text = "SELECT 1;\n\nSELECT 2;";
    let mut b = bar("highlight", text);
    paint(&mut b.bar, 1100.0);
    // A console with SQL follows its text anyway (checked against the schema).
    assert!(b.bar.follows_editor());
    b.bar.hover(Some(center(&b.bar, Tool::Run)));
    assert!(b.bar.follows_editor());
    b.bar.sync_editor(&EditorState { text: text.into(), version: 1, cursor: 12, selection: None });
    assert_eq!(b.bar.highlight(), Some(11..19));
    b.bar.hover(None);
    assert!(b.bar.highlight().is_none());
}

/// The statement the cursor is in is always known (a bar beside its lines), Run hovered or
/// not; hovering Run highlights that same one at once.
#[test]
fn the_statement_under_the_cursor_is_marked() {
    let text = "SELECT 1;\n\nSELECT 2\nFROM t;";
    let mut b = bar("current", text);
    paint(&mut b.bar, 1100.0);
    assert!(b.bar.follows_editor());
    b.bar.sync_editor(&EditorState { text: text.into(), version: 1, cursor: 3, selection: None });
    assert_eq!(b.bar.current(), Some(0..8));
    assert!(b.bar.highlight().is_none());
    b.bar.sync_editor(&EditorState { text: text.into(), version: 1, cursor: 14, selection: None });
    assert_eq!(b.bar.current(), Some(11..26));
    b.bar.hover(Some(center(&b.bar, Tool::Run)));
    assert_eq!(b.bar.highlight(), Some(11..26));
}

/// A run's state shown: what it did, its marks; Stop on while it runs; the last run's time
/// kept with the console.
#[test]
fn what_a_run_did_is_shown() {
    let text = "SELECT * FROM users;";
    let mut b = bar("status", text);
    let runner = b.bar.runner().clone();
    let connector = b.access.connector(b.access.shop, &Overrides::default()).unwrap();
    let statements = match database::console::plan(&PlanInput { text, ..PlanInput::default() }) {
        RunPlan::Run(s) => s,
        other => panic!("{other:?}"),
    };
    runner.run(crate::runner::RunRequest {
        statements,
        document: text.into(),
        settings: Default::default(),
        connector,
        source_name: "shop".into(),
        detached: false,
    });
    assert!(b.bar.poll());
    paint(&mut b.bar, 1100.0);
    assert!(b.bar.enabled(Tool::Stop) && b.bar.is_loading());
    assert_eq!(b.bar.status().unwrap().0, i18n::t("console.running"));
    b.manual.run_all();
    assert!(b.bar.poll());
    let (status, error) = b.bar.status().unwrap();
    assert!(!error && status.contains('3'), "{status}");
    assert_eq!(b.bar.marks().0.len(), 1);
    assert!(!b.bar.is_loading() && !b.bar.enabled(Tool::Stop));
    assert!(b.consoles.spec(b.bar.console()).unwrap().last_run_ms.is_some());
}

/// A transaction left open: the tab asks before it closes (the module's command), never
/// silently.
#[test]
fn closing_with_a_transaction_open_asks_first() {
    let text = "BEGIN; DELETE FROM users;";
    let mut b = bar("closing", text);
    assert!(b.bar.may_close(), "nothing open");
    let runner = b.bar.runner().clone();
    let connector = b.access.connector(b.access.shop, &Overrides::default()).unwrap();
    let statements = match database::console::plan(&PlanInput {
        text,
        mode: RunMode::Script,
        ..PlanInput::default()
    }) {
        RunPlan::Run(s) => s,
        other => panic!("{other:?}"),
    };
    runner.run(crate::runner::RunRequest {
        statements,
        document: text.into(),
        settings: Default::default(),
        connector,
        source_name: "shop".into(),
        detached: false,
    });
    b.manual.run_all();
    assert!(!b.bar.may_close());
    assert!(matches!(b.bar.take_action(), Some(ToolbarAction::View(ViewerAction::WithHost(_)))));
    paint(&mut b.bar, 1100.0);
    assert!(b.bar.hits.iter().any(|(_, t)| *t == Tool::Commit), "Commit and Rollback shown");
    runner.abandon();
    assert!(b.bar.may_close());
}

/// The history's button opens the history window (its command).
#[test]
fn the_history_button_opens_the_history_window() {
    let mut b = bar("history", "SELECT 1;");
    assert!(b.bar.command("more:history"));
    assert_eq!(
        b.bar.take_action(),
        Some(ToolbarAction::View(ViewerAction::RunCommand(crate::HISTORY.0.into())))
    );
}

/// An error underlined in the text follows the editor until its statement is edited: then
/// it's let go.
#[test]
fn an_error_s_underline_goes_once_its_statement_is_edited() {
    let mut b = bar("underline", "SELECT * FORM users;");
    let runner = b.bar.runner().clone();
    let events = kernel::EventBus::new();
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let into = Arc::clone(&heard);
    let _s = events.subscribe(move |e: &workbench_model::DiagnosticsFound| {
        into.lock().unwrap().push(e.diagnostics.len());
    });
    runner.set_context(crate::runner::RunnerContext {
        events: Some(events),
        relative: Some("console.sql".into()),
        ..Default::default()
    });
    let _lease = runner.lease();
    let text = "SELECT * FORM users;";
    let statements = match database::console::plan(&database::console::PlanInput {
        text,
        mode: database::console::RunMode::Script,
        flavor: database::sql::Flavor::Sqlite,
        ..Default::default()
    }) {
        database::console::RunPlan::Run(s) => s,
        _ => Vec::new(),
    };
    assert!(runner.run(crate::runner::RunRequest {
        statements,
        document: text.into(),
        settings: Default::default(),
        connector: b.access.connector(b.access.shop, &Default::default()).unwrap(),
        source_name: "shop".into(),
        detached: false,
    }));
    b.manual.run_all();
    b.bar.poll();
    assert!(b.bar.follows_editor(), "it follows the text while the error is shown");
    assert_eq!(*heard.lock().unwrap(), [1]);
    let state =
        |text: &str| EditorState { text: text.into(), version: 2, cursor: 0, selection: None };
    b.bar.sync_editor(&state(text));
    assert_eq!(*heard.lock().unwrap(), [1], "the same text: kept");
    b.bar.sync_editor(&state("SELECT * FROM users;"));
    assert_eq!(*heard.lock().unwrap(), [1, 0], "edited: let go");
    b.bar.poll();
    assert!(b.bar.follows_editor(), "its text is still checked");
    let _ = &b.consoles;
}

/// Runs `text` (a script) on the bar's console in `tx`.
fn run_script(b: &mut Bar, text: &str, tx: TxMode) {
    let connector = b.access.connector(b.access.shop, &Overrides::default()).unwrap();
    let statements = match database::console::plan(&PlanInput {
        text,
        mode: RunMode::Script,
        ..PlanInput::default()
    }) {
        RunPlan::Run(s) => s,
        other => panic!("{other:?}"),
    };
    let settings = crate::runner::RunSettings { tx, ..Default::default() };
    b.bar.runner().run(crate::runner::RunRequest {
        statements,
        document: text.into(),
        settings,
        connector,
        source_name: "shop".into(),
        detached: false,
    });
    b.manual.run_all();
}

/// An open transaction: `TX` on the tab, what waits for Commit in the pill, Commit and
/// Rollback shown; failed or ended, said so.
#[test]
fn an_open_transaction_marks_the_tab_and_the_pill() {
    let mut b = bar("tx-mark", "");
    let palette = Palette::dark();
    assert!(b.bar.tab_badge(&palette).is_none());
    run_script(
        &mut b,
        "DELETE FROM orders WHERE id = 1; DELETE FROM orders WHERE id = 2;",
        TxMode::Manual,
    );
    assert!(b.bar.poll());
    let (badge, color) = b.bar.tab_badge(&palette).unwrap();
    assert_eq!(badge, i18n::t("console.tab_tx"));
    assert_eq!(color, palette.warning);
    assert!(b.bar.tx_label().ends_with('2'), "{}", b.bar.tx_label());
    paint(&mut b.bar, 1100.0);
    let tip = b.bar.tooltip_at(center(&b.bar, Tool::Tx)).unwrap();
    assert!(tip.label.contains('2'), "{}", tip.label);
    assert!(b.bar.hits.iter().any(|(_, t)| *t == Tool::Commit));
    b.bar.runner().finish_transaction(false);
    b.manual.run_all();
    b.bar.poll();
    assert!(b.bar.tab_badge(&palette).is_none());
    assert!(!b.bar.tx_label().ends_with('2'));
    // Failed (as PostgreSQL gives a transaction up): red, and said.
    b.bar.snapshot.tx = TxState::Failed;
    assert_eq!(b.bar.tab_badge(&palette).unwrap().1, palette.danger);
    assert_eq!(b.bar.tx_label(), i18n::t("console.tx_label_failed"));
}

/// A production source: its badge on the bar (whatever the width), with why.
#[test]
fn a_production_source_is_marked_on_the_bar() {
    let mut b = bar_with("prod-bar", "", |a| a.production = true);
    for width in [1200.0, 520.0] {
        paint(&mut b.bar, width);
        let at = center(&b.bar, Tool::Production);
        let tip = b.bar.tooltip_at(at).unwrap();
        assert_eq!(tip.label, i18n::t("console.production_detail"));
        assert!(b.bar.pointer_down(at).is_none(), "a badge, not a button");
    }
    let mut calm = bar("calm-bar", "");
    paint(&mut calm.bar, 1200.0);
    assert!(!calm.bar.hits.iter().any(|(_, t)| *t == Tool::Production));
}

/// A profile's finding chosen: its place lit in the console while the statement profiled is
/// still the same; once edited, nothing (the mark let go).
#[test]
fn a_finding_lights_its_place_while_its_text_is_the_same() {
    let text = "SELECT * FROM users WHERE id = 1;";
    let mut b = bar("focus", text);
    let runner = b.bar.runner().clone();
    let hash = ui_components::editor_toolbar::text_hash(text);
    runner.set_focus(Some(crate::runner::FocusMark {
        statement: 0..text.len(),
        hash,
        range: 26..32,
        level: ui_components::Level::Critical,
    }));
    assert!(b.bar.poll());
    assert!(b.bar.follows_editor());
    b.bar.sync_editor(&EditorState { text: text.into(), version: 1, cursor: 0, selection: None });
    // Lit in its severity's color, apart from Run's highlight.
    assert_eq!(b.bar.focus(), Some((26..32, ui_components::Level::Critical)));
    assert_eq!(b.bar.highlight(), None);
    // The statement edited: nothing lit, the mark let go.
    let edited = "SELECT * FROM users WHERE id = 12;";
    b.bar.sync_editor(&EditorState { text: edited.into(), version: 2, cursor: 0, selection: None });
    assert_eq!(b.bar.focus(), None);
    assert!(runner.focus().is_none());
    assert!(b.bar.poll());
    assert!(b.bar.follows_editor(), "its text is still checked");
}

/// The ⚙ menu's profiling options: each a switch, saved through the settings command.
#[test]
fn the_settings_menu_has_the_profile_options() {
    let b = bar("profile-options", "SELECT 1;");
    let menu = b.bar.settings_menu();
    let ViewMenuEntry::Submenu { entries, .. } = menu.last().unwrap() else { panic!("{menu:?}") };
    let ids: Vec<&str> = entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        [
            "profile-option:buffers",
            "profile-option:timing",
            "profile-option:verbose",
            "profile-option:statement_stats"
        ]
    );
}
