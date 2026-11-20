//! Testing a connection in the background (connected, refused, cancelled), Apply through
//! the catalog with the passwords kept by policy — the project's vault encrypted at rest,
//! nothing of them in the sources' file — and the modal asking before changes are lost.

use std::sync::Arc;

use database::{
    Capabilities, DbError, Kind, Language, Node, Outcome, RowSink, Scope, Session, Target,
};
use kernel::{EventBus, ProjectId, SecretStore};

use super::*;
use crate::credentials::{MemoryStore, SourceVaults};
use crate::fake::Manual;
use crate::sources::modal::SourcesView;

/// A server that takes one password, and lists two schemas.
struct Server;

struct Open;

impl Session for Open {
    fn server(&self) -> String {
        "PostgreSQL 16.4".into()
    }

    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        let node = |name: &str| Node {
            kind: "schema",
            name: name.into(),
            detail: String::new(),
            path: vec![name.into()],
            expandable: true,
        };
        Ok(if path.is_empty() { vec![node("public"), node("billing")] } else { Vec::new() })
    }

    fn execute(
        &mut self,
        _q: &str,
        _s: &mut dyn RowSink,
        _c: &database::Cancel,
    ) -> Result<Outcome, DbError> {
        Ok(Outcome::default())
    }
}

impl Driver for Server {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { schemas: true, ..Capabilities::default() }
    }

    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        if target.host == "down" {
            return Err(DbError::Connect("timed out".into()));
        }
        if secret.expose() != "right" {
            return Err(DbError::Auth("password authentication failed".into()));
        }
        Ok(Box::new(Open))
    }
}

fn source(host: &str) -> DataSource {
    let mut source = DataSource::new("shop", "server", Scope::Project);
    source.target.host = host.into();
    source
}

#[test]
fn a_test_connects_in_the_background() {
    let manual = Manual::new();
    let tester = Tester::default();
    let credentials = Arc::new(Credentials::default());
    let start = |host: &str, typed: Option<&str>| {
        tester.start(
            &manual.runtime(),
            Arc::new(Server),
            source(host),
            typed.map(Secret::new),
            Arc::clone(&credentials),
            Arc::new(|| {}),
        );
    };
    start("db", Some("right"));
    assert_eq!(tester.probe(), Probe::Running, "nothing on this thread");
    manual.run_all();
    match tester.probe() {
        Probe::Connected { server, schemas, .. } => {
            assert_eq!(server, "PostgreSQL 16.4");
            assert_eq!(schemas, ["public", "billing"]);
        }
        other => panic!("{other:?}"),
    }
    start("db", Some("wrong"));
    manual.run_all();
    assert!(
        matches!(tester.probe(), Probe::Failed(why) if why.contains("password authentication failed"))
    );
    start("down", Some("right"));
    manual.run_all();
    assert!(matches!(tester.probe(), Probe::Failed(why) if why.contains("timed out")));
    // Cancelled: its outcome, when it comes, is forgotten.
    start("db", Some("right"));
    tester.cancel();
    manual.run_all();
    assert_eq!(tester.probe(), Probe::Cancelled);
}

#[test]
fn a_test_uses_the_password_as_kept_when_none_is_typed() {
    let manual = Manual::new();
    let tester = Tester::default();
    let credentials = Arc::new(Credentials::default());
    let shop = source("db");
    credentials.set(&shop, &Secret::new("right")).unwrap();
    tester.start(&manual.runtime(), Arc::new(Server), shop, None, credentials, Arc::new(|| {}));
    manual.run_all();
    assert!(matches!(tester.probe(), Probe::Connected { .. }));
}

struct Project {
    manual: Manual,
    sessions: Arc<Sessions>,
    dir: std::path::PathBuf,
    keys: Arc<MemoryStore>,
}

fn project(name: &str) -> Project {
    database::drivers::register(database::DriverFactory::new(
        "server",
        "Server",
        Kind::Relational,
        Some(1),
        || Arc::new(Server),
    ));
    let dir = std::env::temp_dir().join(format!("sources-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manual = Manual::new();
    let sessions = Sessions::new(ProjectId::new(), &dir, EventBus::new(), manual.runtime(), None);
    sessions
        .set_drivers(Arc::new(|id| (id == "server").then(|| Arc::new(Server) as Arc<dyn Driver>)));
    // The real file of sources (in the temp folder), memory for the keychain.
    sessions
        .catalog()
        .set_store(Arc::new(storage::TomlDataSources::at(Some(&dir), Some(dir.join("config")))));
    let keys = Arc::new(MemoryStore::default());
    let vault =
        SourceVaults { dir: dir.join("vaults"), keys: Arc::clone(&keys) as Arc<dyn SecretStore> };
    sessions.set_credentials(Arc::new(Credentials::new(
        Arc::new(vault),
        Arc::new(MemoryStore::default()),
    )));
    sessions.catalog().ensure_loaded();
    manual.run_all();
    Project { manual, sessions, dir, keys }
}

fn read_all(dir: &Path) -> String {
    let mut out = String::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.push_str(&read_all(&path));
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            out.push_str(&text);
        }
    }
    out
}

#[test]
fn apply_keeps_the_password_encrypted_and_out_of_the_sources_file() {
    let p = project("apply");
    let mut editor = Editor::new(vec![info("server", "Server", &Server)], &[], &[]);
    editor.add("server");
    let host = Field::Spec("host".into());
    editor.input_mut(&host).unwrap().insert("db.example.com");
    editor.edited(&host);
    editor.input_mut(&Field::Password).unwrap().insert("Sup3r-Secret!");
    editor.edited(&Field::Password);
    let plan = editor.plan();
    let done: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let slot = Arc::clone(&done);
    apply(&p.sessions, &plan, Arc::new(move |r| *slot.lock().unwrap() = Some(r))).unwrap();
    assert_eq!(p.sessions.catalog().all().len(), 1, "in the catalog now");
    p.manual.run_all();
    assert_eq!(*done.lock().unwrap(), Some(Ok(())));
    let added = p.sessions.catalog().all().remove(0);
    assert_eq!(
        p.sessions.credentials().get(&added).as_ref().map(Secret::expose),
        Some("Sup3r-Secret!")
    );
    let written = read_all(&p.dir);
    assert!(written.contains("db.example.com"), "the source is written");
    assert!(!written.contains("Sup3r"), "never the password, anywhere in the project");
    assert_eq!(p.keys.len(), 1, "the vault's key in the keychain");
    // Removed: its vault goes too.
    let mut editor =
        Editor::new(vec![info("server", "Server", &Server)], std::slice::from_ref(&added), &[]);
    editor.remove();
    apply(&p.sessions, &editor.plan(), Arc::new(|_| {})).unwrap();
    p.manual.run_all();
    assert!(p.sessions.catalog().all().is_empty());
    assert_eq!(p.sessions.credentials().get(&added), None);
    assert!(p.keys.is_empty());
    let _ = std::fs::remove_dir_all(&p.dir);
}

#[test]
fn the_modal_asks_before_changes_are_lost() {
    let p = project("modal");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    assert!(view.editor().is_some(), "the lists were read");
    assert!(view.may_close(), "nothing to lose: closed");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    assert!(!view.may_close(), "asked first");
    assert!(view.asks_to_discard());
    assert_eq!(view.take_action(), None);
    view.discard();
    assert_eq!(view.take_action(), Some(ui_components::ViewerAction::Close));
    assert!(p.sessions.catalog().all().is_empty(), "nothing applied");
    // Applied: nothing to ask any more.
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    assert!(view.apply());
    assert!(view.may_close());
    let _ = std::fs::remove_dir_all(&p.dir);
}

#[test]
fn a_password_move_is_said() {
    let mut from = source("db");
    from.secrets = SecretPolicy::ProjectVault;
    let mut to = from.clone();
    to.scope = Scope::Global;
    let plan = Plan { secrets: vec![SecretOp::Move { from, to }], ..Plan::default() };
    let notes = move_notes(&plan);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("shop"));
}

#[test]
fn folders_made_from_the_lists_menu_reach_the_catalog_on_apply() {
    let p = project("folders");
    let mut shop = source("db");
    shop.name = "shop".into();
    p.sessions.catalog().add(shop.clone()).unwrap();
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    assert!(view.command("ds:top-folder:project"), "New Folder");
    assert_eq!(view.focus(), Some(&Field::FolderName), "its name to type");
    view.type_text("prod");
    let folder = view.editor().unwrap().selected_folder.unwrap();
    assert!(view.command(&format!("ds:folder-color:{folder}:red")));
    assert!(view.command(&format!("ds:source-move:{}:{folder}", shop.id)));
    let editor = view.editor().unwrap();
    let at = editor.entries.iter().position(|e| e.source.id == shop.id).unwrap();
    assert_eq!(editor.color_of(at), Some(database::SourceColor::Red));
    assert!(p.sessions.catalog().folders().is_empty(), "nothing before Apply");
    assert!(view.apply());
    let catalog = p.sessions.catalog();
    assert_eq!(catalog.folders().len(), 1);
    assert_eq!(catalog.folders()[0].name, "prod", "typed over the new name");
    assert_eq!(catalog.get(shop.id).unwrap().folder, Some(folder));
    assert_eq!(catalog.color_of(shop.id), Some(database::SourceColor::Red));
    // Taken away from the menu: the source stays, at the top.
    assert!(view.command(&format!("ds:folder-delete:{folder}")));
    assert!(view.apply());
    assert!(catalog.folders().is_empty());
    assert_eq!(catalog.get(shop.id).unwrap().folder, None);
    p.manual.run_all();
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// The modal drawn once (its parts' places and tooltips known).
fn paint_modal(view: &mut SourcesView) {
    use ui_components::Viewer as _;
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ui_components::ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1100.0, 760.0),
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
fn every_icon_button_of_the_modal_has_a_tooltip() {
    use crate::sources::form::Tab;
    use crate::sources::modal::Hit;
    use ui_components::Viewer as _;
    let p = project("tips");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    paint_modal(&mut view);
    let icon_only = |hit: &Hit| {
        matches!(
            hit,
            Hit::Tool(_)
                | Hit::SourceColor(Some(_))
                | Hit::FolderColor(Some(_))
                | Hit::Reveal
                | Hit::RemoveOption(_)
        )
    };
    let check = |view: &SourcesView| {
        let mut count = 0;
        for (r, hit) in view.hits.iter().filter(|(_, h)| icon_only(h)) {
            let at =
                kernel::Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
            let tip = view.tooltip_at(at).unwrap_or_else(|| panic!("no tooltip on {hit:?}"));
            assert!(!tip.label.trim().is_empty(), "{hit:?}");
            count += 1;
        }
        count
    };
    // The tools (all five, a source selected), the named colors, the password's eye.
    let general = check(&view);
    assert_eq!(general, 5 + database::SourceColor::NAMED.len() + 1, "{general}");
    let tools = view.hits.iter().filter(|(_, h)| matches!(h, Hit::Tool(_))).count();
    assert_eq!(tools, 5);
    // An option's −, on Advanced.
    let editor = view.editor_mut().unwrap();
    editor.tab = Tab::Advanced;
    editor.add_option();
    paint_modal(&mut view);
    assert!(view.hits.iter().any(|(_, h)| matches!(h, Hit::RemoveOption(0))));
    check(&view);
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// The `+`'s drivers in the shared picker: ↓ ↵ adds the next one; Esc closes it first.
#[test]
fn a_list_of_the_modal_is_the_shared_picker() {
    use crate::sources::modal::Choice;
    use ui_components::{Viewer as _, ViewerKey};
    let p = project("picker");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.open_popup(Choice::AddDriver);
    paint_modal(&mut view);
    assert!(view.picker.is_open());
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(!view.picker.is_open() && view.popup.is_none(), "Esc closes the list, not the modal");
    assert_eq!(view.take_action(), None);
    let before = view.editor().map_or(0, |e| e.entries.len());
    view.open_popup(Choice::AddDriver);
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(view.editor().map_or(0, |e| e.entries.len()), before + 1, "a source added");
    assert!(!view.picker.is_open());
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// A source dragged in the list: its card follows the mouse (its name on it), gone once let go.
#[test]
fn a_dragged_source_follows_the_mouse() {
    use crate::sources::modal::Hit;
    use ui_components::Viewer as _;
    let p = project("drag");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    paint_modal(&mut view);
    let row = view.hits.iter().find(|(_, h)| matches!(h, Hit::Row(_))).map(|(r, _)| *r).unwrap();
    let from = kernel::Point::new(row.min_x() + 40.0, row.min_y() + row.size.height / 2.0);
    view.click(from);
    assert!(view.dragged_card(&ui_components::Palette::dark()).is_none(), "a press isn't a drag");
    assert!(view.drag(kernel::Point::new(from.x + 30.0, from.y + 40.0)));
    let (_, _, name) = view.dragged_card(&ui_components::Palette::dark()).expect("its card");
    let shown = view.editor().unwrap().entries[0].source.name.clone();
    assert_eq!(name, if shown.is_empty() { i18n::t("datasources.unnamed") } else { shown });
    assert_eq!(view.drag_at, kernel::Point::new(from.x + 30.0, from.y + 40.0));
    paint_modal(&mut view);
    view.release();
    assert!(view.dragged_card(&ui_components::Palette::dark()).is_none());
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// A form taller than its room scrolls by its thumb: pressed and dragged down, it moves (no
/// field gets the keys).
#[test]
fn the_form_s_thumb_is_dragged() {
    use ui_components::Viewer as _;
    let p = project("thumb");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    view.paint(ui_components::ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1100.0, 320.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert_eq!(view.form_scroll, 0.0);
    let focus = view.focus.clone();
    // Along the form's right edge (its lane reaches past the fields).
    let area = view.form_area;
    let on = (area.min_y() as u32..area.max_y() as u32)
        .flat_map(|y| (area.min_x() as u32..1100).map(move |x| (x, y)))
        .map(|(x, y)| kernel::Point::new(x as f32, y as f32))
        .find(|p| view.form_bar.contains(*p))
        .expect("the form's scrollbar");
    let at = kernel::Point::new(on.x + 1.0, on.y + 2.0);
    assert_eq!(view.click(at), None);
    assert!(view.drag(kernel::Point::new(at.x, at.y + 30.0)));
    assert!(view.form_scroll > 0.0, "the form moved");
    assert!(view.release());
    assert_eq!(view.focus, focus, "no field took it");
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// The wheel goes to the part under the mouse, through the window's controls: an open
/// list over everything, else the sources' list over it, else the form.
#[test]
fn the_wheel_goes_to_the_part_under_the_mouse() {
    use crate::sources::modal::Choice;
    use ui_components::Viewer as _;
    let p = project("wheel");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    for _ in 0..40 {
        view.editor_mut().unwrap().add("server");
    }
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    view.paint(ui_components::ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1100.0, 320.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let list = view.list_area;
    view.hover(kernel::Point::new(list.min_x() + 20.0, list.min_y() + 20.0));
    assert!(view.scroll(0.0, 30.0));
    assert!(view.list_scroll > 0.0 && view.form_scroll == 0.0, "the list moved");
    let form = view.form_area;
    view.hover(kernel::Point::new(form.min_x() + 40.0, form.min_y() + 40.0));
    let listed = view.list_scroll;
    assert!(view.scroll(0.0, 30.0));
    assert!(view.form_scroll > 0.0 && view.list_scroll == listed, "the form moved");
    // A list opened under the mouse (it hasn't moved since): the list's wheel, nothing else.
    view.open_popup(Choice::AddDriver);
    paint_modal(&mut view);
    let (listed, formed) = (view.list_scroll, view.form_scroll);
    view.scroll(0.0, 30.0);
    assert_eq!((view.list_scroll, view.form_scroll), (listed, formed));
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// Tab (⇧Tab back) and ↑ ↓ move the keys along a driver's fields in the editor's order,
/// around the ends, the text typed going to the one reached; on the filter's row ↑ ↓ move
/// the selection instead, the keys staying in the filter.
#[test]
fn tab_and_the_arrows_follow_the_fields_and_the_filter_row() {
    use ui_components::{KeyReach, Viewer as _, ViewerKey};
    let p = project("order");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    view.editor_mut().unwrap().add("server");
    let order = view.editor().unwrap().texts();
    assert!(order.len() > 3 && order.contains(&Field::Password), "{order:?}");
    view.focus = Some(order[0].clone());
    for field in order.iter().skip(1).chain(order.first()) {
        assert!(view.edit_key(ViewerKey::Tab));
        assert_eq!(view.focus(), Some(field), "Tab: the next, around the end");
    }
    let back = KeyReach { word: false, extend: true };
    assert!(view.edit_key_with(ViewerKey::Tab, back));
    assert_eq!(view.focus(), order.last(), "⇧Tab: the one before");
    assert!(view.edit_key(ViewerKey::Down));
    assert_eq!(view.focus(), order.first(), "↓ as Tab in the form");
    assert!(view.edit_key(ViewerKey::Up));
    assert_eq!(view.focus(), order.last(), "↑ as ⇧Tab");
    // The keys went along: typed into the field reached.
    assert!(view.edit_key(ViewerKey::Down) && view.edit_key(ViewerKey::Down));
    assert_eq!(view.focus(), Some(&Field::Comment));
    view.type_text("a note");
    assert_eq!(view.editor().unwrap().comment.text(), "a note");
    // The filter's row: the selection moves, the keys stay in the filter.
    view.focus = Some(Field::Filter);
    let before = view.editor().unwrap().selected;
    assert!(view.edit_key(ViewerKey::Up));
    assert_ne!(view.editor().unwrap().selected, before, "another source selected");
    assert_eq!(view.focus(), Some(&Field::Filter));
    assert!(view.edit_key(ViewerKey::Down));
    assert_eq!(view.editor().unwrap().selected, before);
    view.type_text("zz");
    assert_eq!(view.editor().unwrap().filter.text(), "zz");
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// Esc in the filter clears it first (the modal stays); once empty, Esc closes (here it
/// asks first: a source was added).
#[test]
fn esc_clears_the_filter_first() {
    use ui_components::{Viewer as _, ViewerKey};
    let p = project("esc");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    view.focus = Some(Field::Filter);
    view.type_text("shop");
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.editor().unwrap().filter.text(), "", "cleared");
    assert!(!view.asks_to_discard() && view.take_action().is_none(), "still open");
    assert_eq!(view.focus(), Some(&Field::Filter));
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(view.asks_to_discard(), "then it's closing");
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// The password is never copied nor cut (the cut is the modal's all the same); another
/// field's selection is, a cut with nothing selected isn't used, a newline typed is dropped.
#[test]
fn the_password_is_never_copied_nor_cut() {
    use ui_components::Viewer as _;
    let p = project("password");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    view.focus = Some(Field::Password);
    view.type_text("hunter2");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text(), None, "never copied");
    assert!(view.command("cut"), "the modal's: nothing behind it gets it");
    assert_eq!(view.editor().unwrap().current().unwrap().password.text(), "hunter2", "not cut");
    view.focus = Some(Field::Comment);
    view.type_text("a\nb");
    assert_eq!(view.editor().unwrap().comment.text(), "ab", "a newline isn't a space");
    assert!(!view.command("cut"), "nothing selected: not used");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("ab"));
    assert!(view.command("cut"));
    assert_eq!(view.editor().unwrap().comment.text(), "");
    assert!(view.command("undo"));
    assert_eq!(view.editor().unwrap().comment.text(), "ab");
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// A press on a field gives it the keys and puts its caret; a second (or third) of a quick
/// series selects no word.
#[test]
fn a_second_click_on_a_field_selects_no_word() {
    use crate::sources::modal::Hit;
    use ui_components::Viewer as _;
    let p = project("clicks");
    let mut view = SourcesView::open(SourcesRequest::new(Arc::clone(&p.sessions)));
    view.editor_mut().unwrap().add("server");
    view.focus = Some(Field::Name);
    view.type_text(" hello world");
    let named = view.editor().unwrap().name.text().to_owned();
    view.focus = None;
    paint_modal(&mut view);
    let field = |view: &SourcesView, f: Field| {
        let r = view.hits.iter().find(|(_, h)| *h == Hit::Field(f.clone())).map(|(r, _)| *r);
        let r = r.unwrap_or_else(|| panic!("{f:?} drawn"));
        kernel::Point::new(r.min_x() + r.size.width / 4.0, r.min_y() + r.size.height / 2.0)
    };
    let name = field(&view, Field::Name);
    assert_eq!(view.press(name, 1), None);
    assert_eq!(view.focus(), Some(&Field::Name), "the keys to it");
    for clicks in [2, 3] {
        view.press(name, clicks);
        view.release();
        assert_eq!(view.selected_text(), None, "{clicks} clicks: nothing selected");
    }
    // The field's part itself, however many clicks it's told of: the caret only.
    let hits = view.hits.clone();
    let input = &mut view.editor_mut().unwrap().name;
    let mut part =
        crate::sources::modal::fields::FieldPart { field: Field::Name, input, hits: &hits };
    for clicks in [2, 3] {
        assert_eq!(
            ui_components::control::Control::press(&mut part, name, clicks),
            ui_components::control::Press::Held
        );
        assert_eq!(part.input.selected_text(), "", "{clicks} clicks: no word, nothing");
    }
    // Another field pressed: the keys go to it.
    let comment = field(&view, Field::Comment);
    view.press(comment, 1);
    view.release();
    view.type_text("x");
    assert_eq!(view.focus(), Some(&Field::Comment));
    assert_eq!(view.editor().unwrap().comment.text(), "x");
    assert_eq!(view.editor().unwrap().name.text(), named, "the name untouched");
    let _ = std::fs::remove_dir_all(&p.dir);
}
