//! A module's hint in the window: shown next to the active tab, floating (the editor keeps
//! its keys, a click elsewhere leaves it), given the keyboard by a click on it, closed for
//! the run by Esc, remembered for good by its action, which runs its command.

use super::*;

/// A project in a scratch folder with a file open, laid out and drawn.
fn opened(name: &str) -> (Workbench, TextSystem) {
    let dir = std::env::temp_dir().join(format!("hints-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "abc\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let mut text = TextSystem::new();
    paint(&mut wb, &mut text);
    (wb, text)
}

fn paint(wb: &mut Workbench, text: &mut TextSystem) {
    wb.paint(text, &mut render_api::Scene::default());
}

/// The terminal's hint (a module's), by its id.
const HINT: &str = terminal_panel::SLEEP_HINT;

#[test]
fn a_hint_floats_by_its_anchor_and_closes_for_the_run() {
    let (mut wb, mut text) = opened("float");
    assert!(!wb.show_hint("no-such-hint"));
    assert!(wb.show_hint(HINT));
    assert!(!wb.show_hint(HINT), "one at a time");
    paint(&mut wb, &mut text);
    let card = wb.hint_card.bounds().expect("drawn");
    let tab = wb.tab_bar.tab_rect(wb.active).unwrap();
    assert!(card.min_y() > tab.max_y(), "under the active tab");
    // Floating: the editor keeps its keys, nothing is blocked.
    assert_eq!(wb.overlay_for_key(&EditKey::Escape), None);
    assert!(!wb.modal_open());
    // A click elsewhere leaves it be.
    let editor = Point::new(600.0, 600.0);
    wb.mouse_down(&mut text, editor, false, false, false, 1);
    assert_eq!(wb.hint_shown(), Some(HINT));
    // A click on it: it has the keyboard; Esc closes it — for this run only.
    let on_card = Point::new(card.min_x() + 20.0, card.min_y() + 40.0);
    wb.mouse_down(&mut text, on_card, false, false, false, 1);
    assert!(wb.hint_focused);
    assert_eq!(wb.overlay_for_key(&EditKey::Escape), Some(Overlay::Hint));
    assert!(Overlay::Hint.slot().key(&mut wb, &mut text, EditKey::Escape));
    assert_eq!(wb.hint_shown(), None);
    assert!(!wb.show_hint(HINT), "not again this run");
    assert!(crate::config::settings().get(&crate::settings::HINTS_SEEN).is_empty(), "not seen");
}

#[test]
fn a_hint_s_action_runs_its_command_and_is_remembered() {
    let (mut wb, mut text) = opened("action");
    assert!(wb.show_hint(HINT));
    paint(&mut wb, &mut text);
    // Its keys: ↵ presses the main one ("Turn on").
    let card = wb.hint_card.bounds().unwrap();
    wb.mouse_down(
        &mut text,
        Point::new(card.min_x() + 20.0, card.min_y() + 40.0),
        false,
        false,
        false,
        1,
    );
    assert!(Overlay::Hint.slot().key(&mut wb, &mut text, EditKey::Enter { shift: false }));
    assert_eq!(wb.take_request(), Some(MouseResult::Command(terminal_panel::SLEEP_ON.0)));
    assert_eq!(wb.hint_shown(), None);
    assert_eq!(crate::config::settings().get(&crate::settings::HINTS_SEEN), [HINT]);
}

#[test]
fn a_hint_s_dont_show_again_is_remembered() {
    let (mut wb, mut text) = opened("dismiss");
    assert!(wb.show_hint(HINT));
    paint(&mut wb, &mut text);
    // Its first button ("Don't show again"), on the left of the footer.
    let card = wb.hint_card.bounds().unwrap();
    let p = (0..card.size.width as i32)
        .map(|x| Point::new(card.min_x() + x as f32, card.max_y() - 25.0))
        .find(|p| wb.hint_card.hit(*p) == Some(ui_workbench::hint_card::CardHit::Button(0)))
        .expect("its button");
    assert_eq!(wb.mouse_down(&mut text, p, false, false, false, 1), MouseResult::Redraw);
    assert_eq!(wb.hint_shown(), None);
    assert_eq!(crate::config::settings().get(&crate::settings::HINTS_SEEN), [HINT]);
}

/// Under the menus and modals: a menu opened over its card takes the click there; a modal
/// hides it, and it comes back once the modal closes.
#[test]
fn a_hint_stays_under_menus_and_modals() {
    let (mut wb, mut text) = opened("under");
    assert!(wb.show_hint(HINT));
    paint(&mut wb, &mut text);
    let card = wb.hint_card.bounds().unwrap();
    let at = Point::new(card.min_x() + 30.0, card.min_y() + 30.0);
    let row = ui_workbench::MenuEntry::Item {
        id: "menu-row".into(),
        label: "A row".into(),
        shortcut: None,
        icon: None,
        on: None,
        look: Default::default(),
    };
    wb.open_context_menu(vec![row], at);
    paint(&mut wb, &mut text);
    assert_ne!(wb.floating_at(at), Some(Overlay::Hint), "the menu's, not the hint's");
    wb.mouse_down(&mut text, Point::new(at.x + 20.0, at.y + 12.0), false, false, false, 1);
    assert!(!wb.hint_focused, "the click was the menu's");
    assert_eq!(wb.hint_shown(), Some(HINT));
    wb.context_menu.close();
    wb.open_about();
    paint(&mut wb, &mut text);
    assert!(!Overlay::Hint.slot().is_open(&wb), "hidden under a modal");
    assert!(wb.hint_card.bounds().is_none());
    assert!(Overlay::About.slot().key(&mut wb, &mut text, EditKey::Escape));
    paint(&mut wb, &mut text);
    assert!(Overlay::Hint.slot().is_open(&wb), "back once it closed");
    assert!(wb.hint_card.bounds().is_some());
}

/// A hint about a tab tints that tab (no ring drawn over it) while it shows, and lets the
/// tint go — fading out — when it closes; a tab not here is never tinted.
#[test]
fn a_hint_about_a_tab_tints_it_while_it_shows() {
    let (mut wb, mut text) = opened("tint");
    let no_such = workbench_model::ViewTab::new("no-such-tab", "txt");
    assert!(!wb.tint_tab(&no_such, Some(workbench_model::TabTone::Accent)));
    let tab = workbench_model::HintAnchor::Tab(workbench_model::ViewTab::new("a", "txt"));
    assert!(wb.show_hint_by(HINT, Some(tab)));
    let now = std::time::Instant::now();
    let id = wb.hint_tab.expect("its tab found");
    assert_eq!(
        wb.tab_tints.of(id, now).map(|(tone, _)| tone),
        Some(workbench_model::TabTone::Accent)
    );
    paint(&mut wb, &mut text);
    assert!(wb.hint_card.bounds().is_some(), "drawn by its tab");
    wb.close_hint(false);
    let later = std::time::Instant::now() + std::time::Duration::from_secs(2);
    assert_eq!(wb.tab_tints.of(id, later), None, "faded out");
}

/// Its tab closed, a hint about it is let go whole (card, tint) without counting as closed
/// by the user: the next tab that asks shows it again — nothing kept alive undrawn.
#[test]
fn a_hint_whose_tab_closes_is_let_go_and_can_show_again() {
    let (mut wb, mut text) = opened("orphan");
    let tab = workbench_model::HintAnchor::Tab(workbench_model::ViewTab::new("a", "txt"));
    assert!(wb.show_hint_by(HINT, Some(tab)));
    let id = wb.hint_tab.expect("its tab found");
    paint(&mut wb, &mut text);
    wb.close_tab(wb.active);
    assert_eq!(wb.hint_shown(), None, "let go with its tab");
    assert!(wb.hint_card.bounds().is_none());
    let later = std::time::Instant::now() + std::time::Duration::from_secs(2);
    assert_eq!(wb.tab_tints.of(id, later), None);
    assert!(wb.show_hint(HINT), "not counted as closed: it shows again");
}

/// Two tabs alike by name are two tabs: a hint about a module's view knows its own by its
/// identity, and a tab not open here gets no hint.
#[test]
fn a_hint_knows_its_tab_by_identity() {
    let (mut wb, _text) = opened("identity");
    let elsewhere = workbench_model::HintAnchor::Tab(workbench_model::ViewTab::new("a", "md"));
    assert!(!wb.show_hint_by(HINT, Some(elsewhere)), "no such view open: not shown");
    let ids: Vec<u128> = wb.tabs.iter().map(|t| t.id).collect();
    assert!(ids.iter().all(|id| *id != 0));
    let tab = workbench_model::HintAnchor::Tab(workbench_model::ViewTab::new("a", "txt"));
    assert!(wb.show_hint_by(HINT, Some(tab)));
    assert_eq!(wb.hint_tab, Some(wb.tabs[wb.active].id));
}
