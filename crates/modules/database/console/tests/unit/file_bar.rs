//! A project's SQL file's bar (39n): "No data source — Attach…" until attached, then the
//! console's bar on its source (the file's own title, icon and breadcrumb kept, the source
//! beside the schema, its menu attaching elsewhere or detaching); every tab of the file
//! following the attachment.

use super::*;
use crate::fake::{FakeAccess, Manual, Project, consoles};
use kernel::Rect;

struct Setup {
    _project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<Consoles>,
    path: PathBuf,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let mut fake = FakeAccess::new(&project);
    fake.archive = Some(DataSourceId::new());
    let access = Arc::new(fake);
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let path = project.root.join("report.sql");
    std::fs::write(&path, "SELECT * FROM users;").unwrap();
    Setup { _project: project, manual, access, consoles, path }
}

fn file_bar(s: &Setup) -> SqlFileBar {
    SqlFileBar::new(Arc::clone(&s.consoles), &s.path, Arc::new(|| {}))
}

fn paint(bar: &mut SqlFileBar, width: f32) {
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

fn attach_button(bar: &SqlFileBar) -> Point {
    let State::Detached(state) = &bar.state else { panic!("attached") };
    let r = state.attach.expect("drawn");
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

/// Attached to nothing: the file's own tab, no color, nothing followed; its Attach button
/// lists the sources speaking SQL, a row attaching it.
#[test]
fn attached_to_nothing_it_offers_the_sources() {
    let s = setup("file-bar-none");
    let mut bar = file_bar(&s);
    assert!(bar.source().is_none() && bar.console_bar().is_none());
    assert_eq!((bar.title(), bar.icon()), (None, None));
    assert!(bar.accent(&Palette::dark()).is_none());
    assert!(bar.crumbs().is_empty() && !bar.follows_editor());
    assert_eq!(bar.height(), HEIGHT);
    paint(&mut bar, 900.0);
    let at = attach_button(&bar);
    assert_eq!(bar.cursor_at(at), Some(ViewerCursor::Pointer));
    assert_eq!(bar.tooltip_at(at).map(|t| t.label), Some(i18n::t("console.tip_attach")));
    assert!(bar.hover(Some(at)) && !bar.hover(Some(at)));
    let Some(ToolbarAction::Menu { entries, .. }) = bar.pointer_down(at) else {
        panic!("its menu")
    };
    let rows: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let archive = s.access.archive.unwrap();
    assert_eq!(rows, [format!("attach:{}", s.access.shop), format!("attach:{archive}")]);
    assert!(bar.pointer_down(Point::new(890.0, 20.0)).is_none(), "nothing else to press");
    // Chosen: the console's bar on the source.
    assert!(bar.command(&rows[0]));
    assert_eq!(bar.source(), Some(s.access.shop));
    assert!(s.consoles.attached_of(&s.path).is_some());
}

/// Attached: the console's bar on its source, with the file's own title, icon and
/// breadcrumb, the source's color, the editor followed (its text checked); the source's
/// pill attaching it elsewhere or detaching it — every bar of the file following.
#[test]
fn attached_it_is_a_console_s_bar_on_its_source() {
    let s = setup("file-bar-attached");
    let mut bar = file_bar(&s);
    let mut other = file_bar(&s);
    s.consoles.attach(&s.path, s.access.shop).unwrap();
    assert!(bar.poll() && other.poll(), "both follow the attachment");
    let console = bar.console_bar().expect("a console's bar");
    assert!(console.runner().leases() > 0);
    assert_eq!((bar.title(), bar.icon()), (None, None), "the file's own tab");
    assert!(bar.crumbs().is_empty());
    assert!(bar.accent(&Palette::dark()).is_some(), "the source's color");
    assert!(bar.follows_editor(), "its text checked against the schema");
    paint(&mut bar, 1100.0);
    let pill = match &bar.state {
        State::Attached(b) => b.hit_of(crate::layout::Tool::Source),
        State::Detached(_) => None,
    };
    let pill = pill.expect("the source's pill");
    let Some(ToolbarAction::Menu { entries, .. }) =
        bar.pointer_down(Point::new(pill.min_x() + 4.0, pill.min_y() + 4.0))
    else {
        panic!("its menu")
    };
    let ids: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let archive = s.access.archive.unwrap();
    assert_eq!(
        ids,
        [format!("source:{}", s.access.shop), format!("source:{archive}"), "detach".into()]
    );
    // Another source: a new console's bar on it, in both tabs.
    assert!(bar.command(&format!("source:{archive}")));
    assert_eq!(bar.source(), Some(archive));
    assert!(other.poll());
    assert_eq!(other.source(), Some(archive));
    // Detached: "No data source" again, the runner let go.
    assert!(bar.command("detach"));
    assert!(bar.source().is_none() && s.consoles.attached_of(&s.path).is_none());
    assert!(other.poll() && other.source().is_none());
    assert!(s.consoles.runners().iter().all(|r| r.leases() == 0));
    s.manual.run_all();
}

/// Attached, the source's pill holds its whole name whatever the scale rounds its width to
/// (never "sh…" for "shop" with the room for it): the glyphs drawn in it are the name's.
#[test]
fn the_source_s_pill_shows_its_whole_name() {
    let s = setup("file-bar-source-name");
    let mut bar = file_bar(&s);
    s.consoles.attach(&s.path, s.access.shop).unwrap();
    assert!(bar.poll());
    let mut text = text_layout::TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let mut scene = render_api::Scene::default();
        bar.paint(ToolbarPaint {
            bounds: Rect::new(0.0, 0.0, 1400.0 * scale, HEIGHT * scale),
            scale,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
        });
        let State::Attached(b) = &bar.state else { panic!("attached") };
        let pill = b.hit_of(crate::layout::Tool::Source).expect("the source's pill");
        let font = ui_components::ds::Ctx::new(&palette, scale).ui_font(-1.0);
        let want: Vec<_> =
            text.shape_line(&b.source_label(), &font).glyphs.iter().map(|g| g.key).collect();
        // The name's glyphs: past its icon (8 + 12 + 6), before its chevron (8 + 12).
        let drawn: Vec<_> = scene
            .layers()
            .iter()
            .flat_map(|l| l.glyphs.iter())
            .filter(|g| {
                pill.contains(g.origin)
                    && g.origin.x >= pill.min_x() + 25.0 * scale
                    && g.origin.x < pill.max_x() - 20.0 * scale
            })
            .map(|g| g.key)
            .collect();
        assert_eq!(drawn, want, "at scale {scale}");
    }
    s.manual.run_all();
}

/// Its file renamed in the project: the same bar follows it (still attached); out of the
/// project's SQL files, the providers are asked again.
#[test]
fn it_follows_its_file() {
    let s = setup("file-bar-moved");
    let mut bar = file_bar(&s);
    let spec = s.consoles.attach(&s.path, s.access.shop).unwrap();
    assert!(bar.poll());
    let to = s.path.with_file_name("renamed.sql");
    std::fs::rename(&s.path, &to).unwrap();
    assert!(bar.moved(&to), "the same bar");
    assert_eq!(bar.source(), Some(s.access.shop));
    assert_eq!(s.consoles.attached_of(&to).map(|a| a.id), Some(spec.id));
    let mut plain = file_bar(&s);
    let txt = to.with_extension("txt");
    assert!(!plain.moved(&txt), "no SQL file now: asked again");
    assert!(plain.moved(&to.with_file_name("x.sql")), "another SQL file of the project");
}

/// The Attach button is a menu button: its chevron, at its right end, is part of its hit and
/// opens the same menu; past it, nothing.
#[test]
fn the_attach_button_s_chevron_opens_its_menu() {
    let s = setup("file-bar-chevron");
    let mut bar = file_bar(&s);
    paint(&mut bar, 900.0);
    let State::Detached(state) = &bar.state else { panic!("attached") };
    let r = state.attach.expect("drawn");
    let chevron = Point::new(r.max_x() - 4.0, r.min_y() + r.size.height / 2.0);
    assert!(matches!(bar.pointer_down(chevron), Some(ToolbarAction::Menu { .. })));
    let past = Point::new(r.max_x() + 4.0, chevron.y);
    assert!(bar.pointer_down(past).is_none());
}
