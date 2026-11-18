//! The profile's view over a console's profile (the sample's, PostgreSQL's made by hand):
//! F2 / ⇧F2 through the problems (each chosen with its step, its place lit in the console),
//! H to the hottest step, the filter typed, a finding ignored for the console, the severity's
//! filter, the layout following the width, Insert that never runs, the exports, Open in Tab.

use super::*;
use crate::fake::{FakeAccess, FakeHost, Manual, Project, consoles};
use render_api::Scene;

struct Setup {
    _project: Project,
    _manual: Manual,
    consoles: Arc<crate::consoles::Consoles>,
    runner: Runner,
    tab: ServiceId,
    file: PathBuf,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, crate::profile::sample::SQL).unwrap();
    let file = spec.path(consoles.root());
    let runner = consoles.runner(spec.id, spec.source);
    let tab = crate::profile::sample::show(&runner, crate::profile::sample::SQL);
    Setup { _project: project, _manual: manual, consoles, runner, tab, file }
}

fn paint(view: &mut ProfileView, width: f32) {
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, width, 700.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

#[test]
fn problems_are_walked_and_lit_in_the_console() {
    let s = setup("profile-problems");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let report = view.report().cloned().unwrap();
    let problems: Vec<usize> =
        (0..report.findings.len()).filter(|&i| report.findings[i].is_problem()).collect();
    assert!(
        problems.len() >= 3,
        "{:?}",
        report.findings.iter().map(|f| f.rule).collect::<Vec<_>>()
    );
    // F2: the first problem, its step chosen, its place lit in the console.
    assert!(view.command("next_problem"));
    assert_eq!(view.selected_finding(), Some(problems[0]));
    let finding = &report.findings[problems[0]];
    assert_eq!(view.selected_node(), finding.nodes.first().copied());
    let focus = s.runner.focus().unwrap();
    assert_eq!(Some(focus.range.clone()), finding.sql_range);
    let sql = crate::profile::sample::SQL;
    assert_eq!(focus.hash, ui_components::editor_toolbar::text_hash(&sql[focus.statement.clone()]));
    // F2 again, ⇧F2 back; round.
    assert!(view.command("next_problem"));
    assert_eq!(view.selected_finding(), Some(problems[1]));
    assert!(view.command("prev_problem"));
    assert!(view.command("prev_problem"));
    assert_eq!(view.selected_finding(), Some(*problems.last().unwrap()));
    // H: the hottest step.
    assert!(view.command("hottest"));
    assert_eq!(view.selected_node(), report.hot_path.last().copied());
    // A finding without a place clears what was lit.
    assert!(s.runner.focus().is_some());
    s.runner.clear_diagnostics();
    assert!(s.runner.focus().is_none(), "a run lets it go");
}

#[test]
fn typing_filters_the_steps() {
    let s = setup("profile-filter");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let all = view.table_mut().visible().len();
    assert!(!view.wants_text());
    for c in "hash".chars() {
        assert!(view.key(c) || view.text_input(&c.to_string()));
    }
    assert!(view.wants_text());
    let shown = view.table_mut().visible();
    assert!(shown.len() < all && !shown.is_empty());
    assert!(shown.iter().any(|&r| view.table().row_text(r).unwrap().contains("Hash")));
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(!view.wants_text());
    assert_eq!(view.table_mut().visible().len(), all);
    // `*` opens everything.
    view.table_mut().set_all_open(false);
    assert!(view.key('*'));
    assert_eq!(view.table_mut().visible().len(), all);
}

#[test]
fn a_rule_is_ignored_for_the_console_and_the_list_filtered() {
    let s = setup("profile-ignore");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let before = view.shown_findings().len();
    let rule = view.shown_findings().iter().find(|f| f.is_problem()).unwrap().rule;
    assert!(view.command(&format!("ignore:{rule}")));
    assert_eq!(s.runner.ignored(), [rule.to_owned()]);
    assert!(view.poll());
    assert_eq!(view.shown_findings().len(), before - 1);
    assert!(!view.shown_findings().iter().any(|f| f.rule == rule));
    // Another view of the same console's profile sees it too; shown again, both see it.
    let other = ProfileView::new(s.runner.clone(), s.tab);
    assert_eq!(other.shown_findings().len(), before - 1);
    s.runner.unignore_all();
    assert!(view.poll());
    assert_eq!(view.shown_findings().len(), before);
    // The severities filter the list.
    assert!(view.command("severity:critical"));
    assert_eq!(view.severity_filter(), Some(Level::Critical));
    assert!(view.command("severity:critical"));
    assert_eq!(view.severity_filter(), None);
}

#[test]
fn the_layout_follows_the_width() {
    let s = setup("profile-layout");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let wide = view.layout_of();
    assert!(wide.mode_wide && wide.panel.size.width > 0.0);
    let wide_cols = view.columns().len();
    paint(&mut view, 700.0);
    let drawer = view.layout_of();
    assert!(!drawer.mode_wide && drawer.panel.size.width == 0.0);
    assert!(view.command("drawer"));
    paint(&mut view, 700.0);
    assert!(view.layout_of().drawer);
    assert!(view.edit_key(ViewerKey::Escape), "Esc closes the drawer");
    paint(&mut view, 450.0);
    let stacked = view.layout_of();
    assert!(stacked.compact.size.height > 0.0 && stacked.switcher_menu);
    assert!(view.columns().len() <= wide_cols);
    paint(&mut view, 2400.0);
    assert!(view.columns().len() >= wide_cols, "more room, more columns");
}

/// Insert puts the SQL at the console's end, selected — nothing runs.
#[test]
fn insert_appends_the_suggestion_without_running_it() {
    let s = setup("profile-insert");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let i = view
        .shown_findings()
        .iter()
        .position(|f| f.suggestion.as_ref().and_then(|x| x.sql()).is_some())
        .unwrap();
    let sql = view.shown_findings()[i].suggestion.as_ref().unwrap().sql().unwrap().to_owned();
    assert!(view.command(&format!("insert:{i}")));
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its insertion") };
    let project = s.consoles.project();
    let mut host = FakeHost::new(project, s.consoles.root());
    work.run(&mut host);
    let (path, text) = host.file.clone().unwrap();
    assert_eq!(path, s.file);
    assert!(text.starts_with(crate::profile::sample::SQL) && text.trim_end().ends_with(&sql));
    let selected = host.selected.clone().unwrap();
    assert_eq!(&text[selected], sql);
    assert!(s.runner.result_tabs().len() == 1 && !s.runner.busy(), "nothing ran");
}

#[test]
fn the_profile_is_exported_and_opened_in_a_tab() {
    let s = setup("profile-export");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    assert!(view.command("profile.export:text"));
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(text))) => assert!(text.contains("Limit")),
        other => panic!("{other:?}"),
    }
    assert!(view.command("profile.export:markdown"));
    assert!(
        matches!(view.take_action(), Some(ViewerAction::Request(AppRequest::CopyText(md))) if md.starts_with("## "))
    );
    assert!(view.command("profile.export:json"));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::PickSavePath { .. }))
    ));
    assert!(!view.take_notifications().is_empty(), "the copies said");
    // Our JSON written in the background, said when done.
    let dir = std::env::temp_dir().join(format!("profile-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.json");
    assert!(view.command(&format!("picked-save:{}", path.display())));
    s._manual.run_all();
    assert!(view.poll());
    assert!(std::fs::read_to_string(&path).unwrap().contains("\"format\""));
    let _ = std::fs::remove_dir_all(&dir);
    // Open in Tab: the editor's tab of the same profile, its breadcrumb and color.
    assert!(view.command("open_in_tab"));
    let Some(ViewerAction::OpenView(opened)) = view.take_action() else {
        panic!("its tab");
    };
    assert_eq!(opened.extension, crate::profile::VIEW);
    assert!(opened.data.downcast_ref::<ProfileTabData>().is_some());
    let set = s.runner.plan(s.tab).unwrap();
    let tab = ProfileView::of(ProfileTabData { runner: s.runner, set });
    assert_eq!(tab.crumbs().len(), 4);
    assert!(tab.report().is_some());
    assert_eq!(tab.actions_view(), Some(crate::profile::VIEW));
    // In the Services window no breadcrumb of its own.
    assert!(view.crumbs().is_empty());
}

/// The tab closed: the view says the profile is gone.
#[test]
fn a_closed_tab_s_view_says_it_s_gone() {
    let s = setup("profile-gone");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1000.0);
    assert!(view.report().is_some());
    s.runner.close_result(s.tab);
    assert!(view.poll());
    assert!(view.report().is_none());
    paint(&mut view, 1000.0);
}

/// A console's real SQLite profile (Explain Analyze on a temporary file) in its tab: its
/// steps as a tree with their measured shares, folded, copied.
#[test]
fn a_console_s_sqlite_profile_is_its_tab() {
    use database::console::{ConsoleAccess, Overrides, PlanInput, RunMode, RunPlan};
    use database::sql;
    let project = Project::new("profile-sqlite");
    let manual = Manual::new();
    let access = FakeAccess::new(&project);
    let runner = Runner::new(database::console::ConsoleId::new(), access.shop, manual.runtime());
    let _lease = runner.lease();
    let text = "SELECT u.name, (SELECT count(*) FROM orders o WHERE o.user_id = u.id) FROM users u ORDER BY 2";
    let RunPlan::Run(mut statements) = database::console::plan(&PlanInput {
        text,
        mode: RunMode::Playground,
        flavor: sql::Flavor::Sqlite,
        ..PlanInput::default()
    }) else {
        panic!()
    };
    let connector = access.connector(access.shop, &Overrides::default()).unwrap();
    assert!(runner.explain(
        statements.remove(0),
        true,
        connector,
        "shop".into(),
        sql::Flavor::Sqlite,
        Arc::from(text)
    ));
    manual.run_all();
    let tab = runner.result_tabs().into_iter().find(|t| t.plan).unwrap();
    assert_eq!(runner.take_reveal(), Some(Some(tab.id)), "shown at once");
    let mut view = ProfileView::new(runner.clone(), tab.id);
    paint(&mut view, 1600.0);
    let n = view.table().rows().len();
    assert!(n >= 3, "{n} steps");
    assert!(view.table().rows().iter().any(|r| r.parent.is_some()));
    assert!(view.columns().contains(&Col::SelfTime), "measured: {:?}", view.columns());
    assert!(view.command("plan.collapse_all"));
    assert!(view.table_mut().visible().len() < n);
    assert!(view.command("plan.expand_all"));
    assert_eq!(view.table_mut().visible().len(), n);
    view.edit_key(ViewerKey::Down);
    assert!(view.selected_text().is_some());
    assert!(view.command("plan.copy"));
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(copied))) => {
            assert!(copied.contains("SCAN u"), "{copied}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        view.status().unwrap(),
        i18n::tr!(
            "console.plan_summary",
            count = n,
            time = millis(runner.plan(tab.id).unwrap().elapsed)
        )
    );
}

/// The tree, the diagram and the flame share the step chosen: a card clicked is the tree's
/// row and the flame's bar, a bar clicked the tree's row and the diagram's card (brought
/// into view); F2 in the diagram chooses the problem's step there too.
#[test]
fn the_three_views_share_the_step_chosen() {
    let s = setup("profile-views");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let report = view.report().cloned().unwrap();
    assert!(ProfileMode::ALL.iter().all(|m| m.available()), "every view made (39h6)");
    // The diagram: a card clicked (pressed and released where it was pressed).
    assert!(view.command("view_graph"));
    assert_eq!(view.mode(), ProfileMode::Graph);
    paint(&mut view, 1400.0);
    let target = report.profile.nodes.iter().position(|n| n.name == "Parallel Hash").unwrap();
    let target = NodeId(target as u32);
    assert!(view.graph_mut().center_on(target));
    paint(&mut view, 1400.0);
    let laid = view.graph().laid().unwrap().clone();
    let card = laid.cards[laid.card_of[target.index()]].rect;
    let at = view.graph().view.rect_to_screen(card);
    let p = Point::new(at.min_x() + at.size.width / 2.0, at.min_y() + at.size.height / 2.0);
    view.pointer_down(p, 1, PressMods::default());
    assert!(view.release());
    assert_eq!(view.selected_node(), Some(target));
    let row = view.table().selected().unwrap();
    assert_eq!(view.table().row_text(row).unwrap().split('\t').next(), Some("Parallel Hash"));
    // The flame follows it.
    assert!(view.command("view_flame"));
    paint(&mut view, 1400.0);
    assert_eq!(view.flame().selected(), Some(target));
    // A bar chosen there by the keys: the tree's choice and the diagram's.
    assert!(view.edit_key(ViewerKey::Up));
    let parent = report.profile.node(target).parent;
    assert_eq!(view.selected_node(), parent);
    // The diagram, F2: the first problem's step, its card in view.
    assert!(view.command("view_graph"));
    assert!(view.command("next_problem"));
    paint(&mut view, 1400.0);
    let chosen = view.selected_node().unwrap();
    let finding = view.shown_findings()[view.selected_finding().unwrap()].nodes[0];
    assert_eq!(chosen, finding);
    let laid = view.graph().laid().unwrap().clone();
    let on = view.graph().view.rect_to_screen(laid.cards[laid.card_of[chosen.index()]].rect);
    assert!(ui_components::diagram::contains_rect(view.graph().view.visible(), on));
    // The arrows walk it; Enter folds the step (shared with the tree).
    assert!(view.edit_key(ViewerKey::Enter) || report.profile.node(chosen).children.is_empty());
    // The zoom: + / − / 0 and the app's zoom.
    let before = view.graph().view.zoom;
    assert!(view.key('+'));
    assert!(view.graph().view.zoom > before);
    assert!(view.key('0'));
    assert_eq!(view.zoom_level(), Some(view.graph().view.zoom));
    assert!(view.command("view_tree"));
    assert_eq!(view.zoom_level(), None);
}

/// The diagram's double click folds a step in the tree too; its "+N" card opens it again.
#[test]
fn folding_is_shared_with_the_tree() {
    let s = setup("profile-fold");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    assert!(view.command("view_graph"));
    paint(&mut view, 1400.0);
    let report = view.report().cloned().unwrap();
    let join = report.profile.nodes.iter().position(|n| n.name == "Parallel Hash Join").unwrap();
    let join = NodeId(join as u32);
    view.graph_mut().center_on(join);
    paint(&mut view, 1400.0);
    let laid = view.graph().laid().unwrap().clone();
    let at = view.graph().view.rect_to_screen(laid.cards[laid.card_of[join.index()]].rect);
    let p = Point::new(at.min_x() + 20.0, at.min_y() + 20.0);
    view.pointer_down(p, 2, PressMods::default());
    view.release();
    let row = view.table().selected();
    let join_row = (0..view.table().rows().len())
        .find(|&r| view.table().row_text(r).unwrap().starts_with("Parallel Hash Join"))
        .unwrap();
    assert!(!view.table().is_open(join_row), "{row:?}");
    paint(&mut view, 1400.0);
    let laid = view.graph().laid().unwrap().clone();
    let fold = laid
        .cards
        .iter()
        .find(|c| matches!(c.item, crate::profile::graph::Item::Folded { .. }))
        .unwrap();
    let at = view.graph().view.rect_to_screen(fold.rect);
    let p = Point::new(at.min_x() + at.size.width / 2.0, at.min_y() + at.size.height / 2.0);
    view.pointer_down(p, 1, PressMods::default());
    view.release();
    assert!(view.table().is_open(join_row));
}

/// The tree's header and the panel's heads are as tall: their lines run on across the
/// splitter (wide), and meet the drawer's (narrower); the flame's band too.
#[test]
fn the_headers_lines_meet_across_the_splitter() {
    let s = setup("profile-heads");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    for scale_width in [1400.0, 1000.0] {
        paint(&mut view, scale_width);
        let tree = view.table().header_rect();
        let [findings, details] = view.panel_heads();
        assert_eq!(tree.max_y(), findings.max_y(), "at {scale_width}");
        assert_eq!(tree.size.height, details.size.height);
        assert_eq!(tree.min_y(), view.layout_of().tree.min_y());
    }
    // The drawer, narrower, over the tree: the same line.
    assert!(view.command("drawer"));
    paint(&mut view, 700.0);
    assert!(view.layout_of().drawer);
    assert_eq!(view.table().header_rect().max_y(), view.panel_heads()[0].max_y());
    // The flame's band is as tall as the tree's header.
    assert_eq!(ui_components::icicle::BAND_H, ui_components::tree_table::HEADER_H);
}

/// The tables' view: a table chosen chooses its first step (and its finding); F2 chooses
/// the problem's table there; its menu opens the table and its structure.
#[test]
fn the_tables_view_shares_the_step_chosen() {
    let project = Project::new("profile-tables");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, crate::profile::sample::SQL).unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let tab = crate::profile::sample::show_tables(&runner, crate::profile::sample::SQL);
    let mut view = ProfileView::new(runner, tab);
    paint(&mut view, 1400.0);
    let jobs = manual.jobs();
    assert!(view.command("view_tables"));
    assert_eq!(view.mode(), ProfileMode::Tables);
    paint(&mut view, 1400.0);
    assert_eq!(manual.jobs(), jobs, "its statistics known: nothing asked");
    let built = view.tables_built().clone();
    assert_eq!(built.usage.len(), 2);
    assert!(built.has_stats);
    // A table chosen (the keys): its first step chosen in the tree.
    assert!(view.edit_key(ViewerKey::Down));
    let row = view.tables().selected().unwrap();
    let TablesRow::Table(u) = built.kinds[row].clone() else { panic!("{:?}", built.kinds[row]) };
    assert_eq!(view.selected_node(), built.usage[u].scans.first().map(|s| s.node));
    // A problem about a table's step chosen: its table chosen here too.
    let report = view.report().cloned().unwrap();
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Down);
    let away = view.tables().selected();
    let at = view
        .shown_findings()
        .iter()
        .position(|f| {
            f.is_problem() && f.nodes.first().is_some_and(|n| report.profile.node(*n).op.is_scan())
        })
        .unwrap();
    assert!(view.command(&format!("finding:{at}")));
    let node = view.selected_node().unwrap();
    let row = view.tables().selected().unwrap();
    assert_ne!(Some(row), away, "moved to its table");
    let TablesRow::Table(u) = built.kinds[row].clone() else { panic!() };
    assert!(built.usage[u].nodes().contains(&node));
    // Its menu: the table, its structure (the fake source opens neither: nothing asked).
    let at = view.tables().header_rect();
    let entries = view.context_menu(Point::new(at.min_x() + 60.0, at.max_y() + 13.0)).unwrap();
    let ids: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert!(
        ids.contains(&"tables.open".to_owned()) && ids.contains(&"tables.structure".to_owned())
    );
    assert!(view.command("tables.open"));
    assert!(view.take_action().is_none());
    // Another view chosen, the tables left.
    assert!(view.command("view_tree"));
    assert_eq!(view.mode(), ProfileMode::Tree);
}

/// A profile shown without its tables' statistics: opening its Tables view reads them once,
/// in the background, on the console's connection (a real SQLite file).
#[test]
fn the_tables_statistics_are_read_when_the_view_opens() {
    use database::console::{ConsoleAccess, Overrides, PlanInput, RunMode, RunPlan};
    use database::sql;
    let project = Project::new("profile-tables-lazy");
    let manual = Manual::new();
    let access = FakeAccess::new(&project);
    let runner = Runner::new(database::console::ConsoleId::new(), access.shop, manual.runtime());
    let _lease = runner.lease();
    let text = "SELECT u.name, o.total FROM users u JOIN orders o ON o.user_id = u.id";
    let RunPlan::Run(mut statements) = database::console::plan(&PlanInput {
        text,
        mode: RunMode::Playground,
        flavor: sql::Flavor::Sqlite,
        ..PlanInput::default()
    }) else {
        panic!()
    };
    let connector = access.connector(access.shop, &Overrides::default()).unwrap();
    assert!(runner.explain(
        statements.remove(0),
        true,
        connector,
        "shop".into(),
        sql::Flavor::Sqlite,
        Arc::from(text)
    ));
    manual.run_all();
    let tab = runner.result_tabs().into_iter().find(|t| t.plan).unwrap();
    let set = runner.plan(tab.id).unwrap();
    let report = set.report.unwrap();
    // As made, its facts hold the statistics (the auxiliary queries on): sqlite_stat1 missing.
    let facts = runner.profile_facts(report.profile.id).unwrap();
    assert!(
        facts
            .tables
            .iter()
            .all(|t| t.stats.as_ref().is_some_and(|s| s.stats_present == Some(false)))
    );
    // The same report shown again without them.
    let bare = database::profile::rules::Facts {
        tables: facts
            .tables
            .iter()
            .map(|t| database::profile::rules::TableFacts { stats: None, ..t.clone() })
            .collect(),
        ..database::profile::rules::Facts::default()
    };
    let mut again = (*report).clone();
    again.profile = Arc::new(database::profile::Profile {
        id: database::profile::ProfileId::new(),
        ..(*report.profile).clone()
    });
    let tab = runner.show_profile(crate::runner::ProfileShown {
        report: Arc::new(again),
        title: "Profile".into(),
        table: None,
        statement: text.into(),
        range: 0..text.len(),
        analyze: true,
        rolled_back: false,
        elapsed: std::time::Duration::from_millis(3),
        document_hash: None,
        facts: Some(Arc::new(bare)),
        fingerprint: 0,
    });
    let mut view = ProfileView::new(runner, tab);
    paint(&mut view, 1400.0);
    assert!(view.command("view_tables"));
    paint(&mut view, 1400.0);
    assert!(!view.tables_built().has_stats);
    assert_eq!(manual.jobs(), 1, "asked once, in the background");
    paint(&mut view, 1400.0);
    assert_eq!(manual.jobs(), 1, "not asked twice");
    manual.run_all();
    assert!(view.poll());
    paint(&mut view, 1400.0);
    assert!(view.tables_built().has_stats, "read and shown");
    assert!(view.tables_built().cols.contains(&crate::profile::tables::TCol::Analyzed));
}

/// Compare ▾: the console's other profiles (the same query's first), one chosen compares
/// the profile shown with it (the older before), swapped, closed with Esc or by another view.
#[test]
fn a_profile_is_compared_with_another_of_its_console() {
    let project = Project::new("profile-compare");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, crate::profile::sample::SQL).unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let (tab, before) = crate::profile::sample::show_compare(&runner, crate::profile::sample::SQL);
    let list = runner.profile_list();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].fingerprint, list[1].fingerprint, "the same query");
    let mut view = ProfileView::new(runner, tab);
    paint(&mut view, 1400.0);
    // Compare ▾ lists the other one, as the same query's.
    assert!(view.command("compare"));
    paint(&mut view, 1400.0);
    assert!(view.comparing().is_none());
    assert!(view.command(&format!("profile.compare:{before}")));
    let c = view.comparing().unwrap();
    assert!(c.shown_after, "the newer is after");
    assert_eq!(c.before.report.profile.id, before);
    assert!(c.diff.shape_changed);
    paint(&mut view, 1400.0);
    // A row chosen: its step of this profile chosen.
    assert!(view.edit_key(ViewerKey::Down));
    let row = view.comparing().unwrap().table.selected().unwrap();
    assert_eq!(view.selected_node(), view.comparing().unwrap().own_step(row));
    // Copied as text, swapped, closed with Esc.
    assert!(view.command("compare.copy"));
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(text))) => assert!(text.contains("→")),
        other => panic!("{other:?}"),
    }
    assert!(view.command("compare.swap"));
    assert!(!view.comparing().unwrap().shown_after);
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(view.comparing().is_none());
    // Not with itself; another view leaves the comparison.
    let own = view.report().unwrap().profile.id;
    assert!(!view.command(&format!("profile.compare:{own}")));
    assert!(view.command(&format!("profile.compare:{before}")));
    assert!(view.command("view_graph"));
    assert!(view.comparing().is_none());
}

/// The view's tooltips (some of several lines: a step's words and kind, the statement);
/// with its own dropdown open, nothing under or behind it shows one.
#[test]
fn no_tooltip_shows_while_the_views_dropdown_is_open() {
    let s = setup("profile-tip-dropdown");
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    paint(&mut view, 1400.0);
    let with_tip: Vec<Point> = (0..140)
        .flat_map(|y| (0..280).map(move |x| Point::new(x as f32 * 5.0, y as f32 * 5.0)))
        .filter(|p| view.tooltip_at(*p).is_some())
        .collect();
    assert!(!with_tip.is_empty());
    // Tooltips of several lines (a step's, the statement's) are there.
    assert!(with_tip.iter().filter_map(|p| view.tooltip_at(*p)).any(|t| t.label.contains('\n')));
    let (export, ..) = *view.hits.iter().find(|(_, h, _)| *h == Hit::Export).unwrap();
    view.click(Point::new(export.min_x() + 4.0, export.min_y() + 4.0));
    assert!(view.dropdown.is_some());
    paint(&mut view, 1400.0);
    assert!(with_tip.iter().all(|p| view.tooltip_at(*p).is_none()));
}

/// The panel's width is the project's: dragged, it's kept in its sizes; the next profile
/// view of the project (or of its next session) is as wide; a profile given to a tab's view
/// keeps them; a double click gives the default back.
#[test]
fn the_panel_s_width_is_kept_in_the_project_s_sizes() {
    let s = setup("profile-width");
    let sizes = workbench_model::LayoutSizes::new();
    let mut view = ProfileView::new(s.runner.clone(), s.tab);
    view.set_data(&sizes);
    paint(&mut view, 1400.0);
    let handle = view.splitter.handle();
    let at = Point::new(handle.min_x() + handle.size.width / 2.0, handle.min_y() + 10.0);
    view.pointer_down(at, 1, PressMods::default());
    assert!(view.drag(Point::new(at.x - 120.0, at.y)));
    view.release();
    let kept = sizes.get(PANEL_SIZE).expect("kept in the project");
    assert_eq!(kept, view.panel_w.round());
    assert!(kept > crate::profile::layout::PANEL_W + 100.0);
    let mut next = ProfileView::new(s.runner.clone(), s.tab);
    assert_eq!(next.panel_w, crate::profile::layout::PANEL_W);
    next.set_data(&sizes);
    assert_eq!(next.panel_w, kept);
    // Double click: the default, forgotten in the project.
    paint(&mut view, 1400.0);
    let handle = view.splitter.handle();
    let at = Point::new(handle.min_x() + handle.size.width / 2.0, handle.min_y() + 10.0);
    view.pointer_down(at, 2, PressMods::default());
    assert_eq!(view.panel_w, crate::profile::layout::PANEL_W);
    assert_eq!(sizes.get(PANEL_SIZE), None);
}
