//! The overlays as they work today (step 17a of plan 17: the safety net before they move
//! onto one contract): every modal keeps everything behind it from getting keys, text,
//! scroll or zoom; a click outside doesn't close it; Esc does; when two are open, the one
//! drawn on top gets the keys first.

use super::*;

/// A project in a scratch folder with a long file open in the editor, laid out.
fn opened(name: &str) -> (Workbench, PathBuf, TextSystem) {
    let dir = std::env::temp_dir().join(format!("overlays-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "abc\n".repeat(400)).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let mut text = TextSystem::new();
    paint(&mut wb, &mut text);
    (wb, dir, text)
}

fn paint(wb: &mut Workbench, text: &mut TextSystem) {
    wb.paint(text, &mut render_api::Scene::default());
}

fn dialog_spec(input: bool) -> ui_workbench::DialogSpec {
    use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
    DialogSpec {
        tone: Tone::Accent,
        icon: "file-pen",
        title: "t".into(),
        message: "m".into(),
        buttons: vec![
            DialogButton::new("cancel", "Cancel".to_owned(), ButtonKind::Secondary),
            DialogButton::new("ok", "Ok".to_owned(), ButtonKind::Primary),
        ],
        cancel: "cancel",
        input: input.then(|| ("name".to_owned(), 0..4)),
        check: None,
        list: Vec::new(),
    }
}

/// Opens modal `o` the way the app does.
fn open(wb: &mut Workbench, o: Overlay) {
    match o {
        Overlay::About => wb.open_about(),
        Overlay::Dialog => wb.open_dialog(dialog_spec(false)),
        Overlay::Settings => wb.open_settings(SettingsModel::default()),
        // Project search: the indexing module's view over the window.
        Overlay::ModalView => assert!(wb.open_project_search(), "search opens"),
        Overlay::ProjectsMenu => wb.open_projects_menu(),
        other => panic!("not a modal: {other:?}"),
    }
}

/// The modals of `CLAUDE.md`: About, dialogs, settings, project search (the indexing
/// module's view), the projects list.
const MODALS: [Overlay; 5] =
    [Overlay::About, Overlay::Dialog, Overlay::Settings, Overlay::ModalView, Overlay::ProjectsMenu];

#[test]
fn a_modal_keeps_everything_behind_it_from_input() {
    for o in MODALS {
        let (mut wb, dir, mut text) = opened(&format!("{o:?}"));
        let before = wb.tabs[wb.active].doc.buffer().to_string();
        open(&mut wb, o);
        paint(&mut wb, &mut text);
        assert!(wb.overlay_is_open(o), "{o:?} opens");
        assert!(wb.modal_open(), "{o:?} is modal: nothing behind it gets commands");
        assert!(wb.keyboard_in_overlay(), "{o:?}: the editor shows no caret");
        wb.type_text(&mut text, "x");
        wb.edit_key(&mut text, EditKey::Backspace { word: false });
        assert!(wb.overlay_is_open(o), "{o:?}: typing doesn't close it");
        let a = wb.editor_area;
        let editor = Point::new(a.min_x() + a.size.width / 2.0, a.min_y() + a.size.height / 2.0);
        assert!(!wb.zoom_by(1.5, editor), "{o:?}: no zoom behind it");
        assert!(!wb.open_project_search(), "{o:?}: no search over it");
        let after = wb.tabs[wb.active].doc.buffer().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(after, before, "{o:?}: the editor got nothing");
    }
}

#[test]
fn a_click_outside_a_modal_does_not_close_it_and_esc_does() {
    for o in MODALS {
        let (mut wb, dir, mut text) = opened(&format!("click-{o:?}"));
        open(&mut wb, o);
        paint(&mut wb, &mut text);
        // The window's bottom left corner: outside every modal.
        let outside = Point::new(6.0, 790.0);
        wb.mouse_down(&mut text, outside, false, false, false, 1);
        wb.mouse_up(&mut text);
        paint(&mut wb, &mut text);
        assert!(wb.overlay_is_open(o), "{o:?}: a stray click keeps it open");
        // A query typed first: Esc clears it, the next one closes.
        for _ in 0..3 {
            if !wb.overlay_is_open(o) {
                break;
            }
            wb.edit_key(&mut text, EditKey::Escape);
            paint(&mut wb, &mut text);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!wb.overlay_is_open(o), "{o:?}: Esc closes it");
    }
}

/// Two open at once: the one drawn on top gets the keyboard (and Esc closes it first).
#[test]
fn the_overlay_drawn_on_top_gets_the_keys_first() {
    // A dialog asked for while About is open is drawn over it.
    let (mut wb, dir, mut text) = opened("about-dialog");
    wb.open_about();
    wb.open_dialog(dialog_spec(true));
    paint(&mut wb, &mut text);
    wb.type_text(&mut text, "s");
    assert_eq!(wb.dialogs.dialog.input.as_ref().map(|i| i.text().to_owned()).as_deref(), Some("s"));
    wb.edit_key(&mut text, EditKey::Escape);
    assert!(!wb.dialog_open(), "Esc closes the dialog on top");
    assert!(wb.overlay_is_open(Overlay::About), "About, under it, stays");
    let _ = std::fs::remove_dir_all(&dir);

    // Settings opened over the projects list.
    let (mut wb, dir, mut text) = opened("projects-settings");
    wb.open_projects_menu();
    wb.open_settings(SettingsModel::default());
    paint(&mut wb, &mut text);
    wb.edit_key(&mut text, EditKey::Escape);
    paint(&mut wb, &mut text);
    assert!(!wb.overlay_is_open(Overlay::Settings), "Esc closes Settings, on top");
    assert!(wb.overlay_is_open(Overlay::ProjectsMenu), "the list under it stays");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The keyboard order is the drawing order (topmost first): `Overlay::ALL` says both.
#[test]
fn the_keyboard_order_is_the_drawing_order() {
    let drawn_bottom_up = [
        // The resources pinned in the editor's corner: under everything.
        Overlay::ResourcePills,
        Overlay::ProjectsMenu,
        Overlay::Settings,
        Overlay::About,
        Overlay::ModalView,
        Overlay::Dialog,
        Overlay::ChoiceMenu,
        // A dropdown's list (a bar's), over what it was opened from.
        Overlay::DropdownList,
        Overlay::ContextMenu,
        // The app's menus from the window's bar: over everything they could open.
        Overlay::MenuDrawer,
        // A status bar item's window floats over all of them, the notifications over it.
        Overlay::StatusPopover,
        // The notifications float over all of them.
        Overlay::Toasts,
        // Files dragged from the file manager: over everything, the notifications too.
        Overlay::FileDrop,
    ];
    let mut top_down = drawn_bottom_up;
    top_down.reverse();
    assert_eq!(Overlay::ALL, top_down);
}

/// Esc goes to what's in front, in order: the modal first (only it), then — once it's
/// closed — what had the keyboard before it (a side panel), which kept its focus.
#[test]
fn esc_goes_to_what_is_in_front_then_to_what_had_the_keyboard() {
    let (mut wb, dir, mut text) = opened("esc-order");
    let dock = wb.tools.sides.first().map(|s| s.dock).expect("the project panel is shown");
    wb.focus = Focus::Side(dock);
    wb.open_settings(SettingsModel::default());
    paint(&mut wb, &mut text);
    assert!(wb.focused_side().is_none(), "the panel doesn't get keys under the modal");
    wb.edit_key(&mut text, EditKey::Escape);
    paint(&mut wb, &mut text);
    assert!(!wb.overlay_is_open(Overlay::Settings), "the first Esc: the modal");
    assert_eq!(wb.focus, Focus::Side(dock), "the panel kept the keyboard");
    assert!(wb.focused_side().is_some(), "the next Esc goes to the panel");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The stack is what's open: a popover (a menu) over a modal gets the keys first; closing
/// each leaves the next, nothing to clean up.
#[test]
fn the_stack_is_what_is_open_and_shrinks_as_things_close() {
    let (mut wb, dir, mut text) = opened("stack");
    wb.open_settings(SettingsModel::default());
    wb.open_dialog(dialog_spec(false));
    paint(&mut wb, &mut text);
    assert_eq!(wb.active_overlay(), Some(Overlay::Dialog));
    wb.edit_key(&mut text, EditKey::Escape);
    assert_eq!(wb.active_overlay(), Some(Overlay::Settings));
    wb.edit_key(&mut text, EditKey::Escape);
    assert_eq!(wb.active_overlay(), None);
    assert!(!wb.modal_open());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Modals and popovers, as the contract says.
#[test]
fn the_kinds_are_modals_and_popovers() {
    use super::super::overlay_stack::OverlayKind::{Modal, Popover};
    let (wb, dir, _text) = opened("kinds");
    let kinds: Vec<_> = Overlay::ALL.into_iter().map(|o| (o, o.slot().kind(&wb))).collect();
    let _ = std::fs::remove_dir_all(&dir);
    for (o, kind) in kinds {
        let popover = matches!(
            o,
            Overlay::ContextMenu
                | Overlay::DropdownList
                | Overlay::ChoiceMenu
                | Overlay::Toasts
                | Overlay::StatusPopover
                | Overlay::ResourcePills
                | Overlay::FileDrop
        );
        assert_eq!(kind, if popover { Popover } else { Modal }, "{o:?}");
    }
}

/// A key goes down the open overlays until one keeps it: today every one keeps its keys;
/// a modal keeps them even under a popover that would let them go.
#[test]
fn a_key_stops_at_the_first_overlay_that_keeps_it() {
    let (mut wb, dir, _text) = opened("key-chain");
    assert_eq!(wb.overlay_for_key(&EditKey::Escape), None, "nothing open: the focus chain");
    wb.open_settings(SettingsModel::default());
    wb.open_dialog(dialog_spec(false));
    assert_eq!(wb.overlay_for_key(&EditKey::Escape), Some(Overlay::Dialog));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Common to every overlay with a search: Esc clears what's typed first, the next one
/// closes; the cursor keys move in its field (←, a word, Home / End).
#[test]
fn esc_clears_a_search_first_and_the_cursor_keys_move_in_the_field() {
    for o in [Overlay::Settings, Overlay::ProjectsMenu] {
        let (mut wb, dir, mut text) = opened(&format!("query-{o:?}"));
        open(&mut wb, o);
        paint(&mut wb, &mut text);
        wb.type_text(&mut text, "tema");
        let field = |wb: &Workbench| match o {
            Overlay::Settings => {
                (wb.settings.query_input().text().to_owned(), wb.settings.query_input().cursor())
            }
            _ => (
                wb.home.projects_menu.query.text().to_owned(),
                wb.home.projects_menu.query.cursor(),
            ),
        };
        assert_eq!(field(&wb), ("tema".to_owned(), 4), "{o:?}");
        let left = EditKey::Move { motion: Motion::Left, extend: false };
        wb.edit_key(&mut text, left);
        assert_eq!(field(&wb).1, 3, "{o:?}: ← moves in the field");
        let home = EditKey::Move { motion: Motion::LineStart, extend: false };
        wb.edit_key(&mut text, home);
        assert_eq!(field(&wb).1, 0, "{o:?}: Home");
        wb.edit_key(&mut text, EditKey::Escape);
        assert!(wb.overlay_is_open(o), "{o:?}: the first Esc clears the search");
        assert_eq!(field(&wb).0, "", "{o:?}");
        wb.edit_key(&mut text, EditKey::Escape);
        assert!(!wb.overlay_is_open(o), "{o:?}: the next one closes it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A module's view bare over the window, as a popover (the colors module's picker): the
/// stack calls it a popover, it gets the keys, Esc closes it; a framed one is a modal.
#[test]
fn a_module_view_opens_bare_as_a_popover() {
    let (mut wb, dir, mut text) = opened("bare");
    let pick = ui_workbench::ColorPick {
        anchor: Rect::new(300.0, 200.0, 12.0, 12.0),
        color: kernel::Rgba::rgb(0x3b82f6),
        format: code_intelligence::ColorFormat::Hex,
        range: 0..0,
    };
    assert!(wb.open_bare_view(ui_workbench::PICKER_EXTENSION, &pick, true));
    paint(&mut wb, &mut text);
    assert_eq!(wb.active_overlay(), Some(Overlay::ModalView));
    assert_eq!(
        Overlay::ModalView.slot().kind(&wb),
        super::super::overlay_stack::OverlayKind::Popover
    );
    assert!(!wb.modal_open(), "a popover: not a modal");
    wb.edit_key(&mut text, EditKey::Escape);
    assert_eq!(wb.active_overlay(), None, "Esc closes it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Full screen with the window's buttons ours (Windows): the top bar stays, as on macOS —
/// the content keeps its place below it, and the bar's middle button leaves full screen.
#[test]
fn full_screen_keeps_the_top_bar() {
    let (mut wb, dir, mut text) = opened("full-screen");
    wb.set_window_controls(Some(false));
    let below_bar = wb.tools.strip.bounds().min_y();
    assert!(below_bar > 0.0);
    wb.set_full_screen(true);
    paint(&mut wb, &mut text);
    assert_eq!(wb.tools.strip.bounds().min_y(), below_bar, "the content stays below the bar");
    let maximize = Point::new(1200.0 - 36.0 - 18.0, 10.0);
    assert_eq!(
        wb.mouse_down(&mut text, maximize, false, false, true, 1),
        MouseResult::Command(crate::commands::FULL_SCREEN.0),
        "its middle button leaves full screen"
    );
    wb.set_full_screen(false);
    assert_eq!(wb.tools.strip.bounds().min_y(), below_bar, "still below the bar");
    let _ = std::fs::remove_dir_all(dir);
}

/// Some place in the window with a tooltip of its own (a bar's button, a tool window's).
fn a_place_with_a_tooltip(wb: &Workbench) -> Point {
    (0..120)
        .flat_map(|y| (0..240).map(move |x| Point::new(x as f32 * 5.0, y as f32 * 6.67)))
        .find(|p| wb.tooltip_at(*p).is_some())
        .expect("something in the window has a tooltip")
}

/// An open context menu (or any window over the editor) keeps every tooltip behind it from
/// showing: none drawn over the menu, none started by the mouse passing under it; once it
/// closes, they come back.
#[test]
fn no_tooltip_shows_behind_an_open_context_menu() {
    let (mut wb, _dir, mut text) = opened("tip-menu");
    let p = a_place_with_a_tooltip(&wb);
    let entries = vec![ui_workbench::MenuEntry::Item {
        id: "noop".into(),
        label: "Compare with Clipboard".into(),
        shortcut: None,
        icon: None,
        on: None,
        look: Default::default(),
    }];
    wb.open_context_menu(entries, Point::new(600.0, 400.0));
    paint(&mut wb, &mut text);
    assert!(wb.overlay_open());
    assert_eq!(wb.tooltip_at(p), None, "the menu is open: nothing behind it has a tooltip");
    wb.hover(p);
    assert_eq!(wb.tooltips.deadline(), None, "no tooltip waits to appear over the menu");
    wb.context_menu.close();
    assert!(!wb.overlay_open());
    assert!(wb.tooltip_at(p).is_some());
    wb.hover(p);
    assert!(wb.tooltips.deadline().is_some());
}

/// A dialog's list, its thumb pressed and dragged: the list follows, until released.
#[test]
fn a_dialog_s_list_follows_its_thumb() {
    let (mut wb, dir, mut text) = opened("dialog-thumb");
    let mut spec = dialog_spec(false);
    spec.list = (0..40).map(|i| format!("src/f{i}.php")).collect();
    wb.open_dialog(spec);
    paint(&mut wb, &mut text);
    let card = wb.dialogs.dialog.card();
    // The list ends at the footer: its thumb, at the top, is a list's height above it.
    let list_top = card.max_y() - 58.0 - (ui_workbench::LIST_ROWS as f32 * 24.0 + 8.0);
    let thumb = Point::new(card.max_x() - 3.0, list_top + 10.0);
    wb.mouse_down(&mut text, thumb, false, false, false, 1);
    assert!(wb.drag_pending(), "the mouse follows the thumb");
    assert!(wb.mouse_drag(&mut text, Point::new(thumb.x, thumb.y + 80.0)));
    let moved = wb.dialogs.dialog.list_offset();
    assert!(moved > 0.0, "the list followed");
    assert!(wb.mouse_up(&mut text));
    assert!(!wb.dialogs.dialog.scrollbar_held(), "released");
    assert!(!wb.drag_pending());
    assert_eq!(wb.dialogs.dialog.list_offset(), moved, "released: it stays");
    assert!(wb.dialogs.dialog.is_open(), "a press on its scrollbar doesn't answer it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A bar's dropdown list: shown by the app as the shared picker, a popover with the keys;
/// a row taken goes back to the bar as its command, and the list closes.
#[test]
fn a_bars_dropdown_list_answers_with_the_rows_command() {
    let (mut wb, dir, mut text) = opened("dropdown-list");
    let row = |label: &str| ui_workbench::picker::PickerItem {
        label: label.to_owned(),
        ..Default::default()
    };
    let action = ui_workbench::editor_toolbar::ToolbarAction::List {
        anchor: kernel::Rect::new(100.0, 40.0, 160.0, 28.0),
        rows: vec![("env:".to_owned(), row("None")), ("env:dev".to_owned(), row("dev"))],
        chosen: Some(0),
    };
    wb.toolbar_action(&mut text, action);
    assert_eq!(wb.active_overlay(), Some(Overlay::DropdownList));
    wb.edit_key(&mut text, EditKey::Move { motion: Motion::Down, extend: false });
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        wb.take_request(),
        Some(MouseResult::MenuCommand(format!("{}env:dev", crate::workbench::BAR_MENU)))
    );
    assert_eq!(wb.active_overlay(), None);
}
