use super::*;
use crate::fake::FakeRepository;
use version_control::RefLabel;

fn label(name: &str, kind: RefKind) -> RefLabel {
    RefLabel { name: name.into(), kind }
}

fn repo() -> FakeRepository {
    FakeRepository {
        refs: vec![
            label("alpha", RefKind::Branch),
            label("main", RefKind::Branch),
            label("zeta", RefKind::Branch),
            label("origin/HEAD", RefKind::Remote),
            label("origin/main", RefKind::Remote),
            label("origin/other", RefKind::Remote),
            label("v1", RefKind::Tag),
        ],
        branch_states: vec![SyncState {
            branch: "main".into(),
            upstream: Some("origin/main".into()),
            ahead: 2,
            behind: 1,
        }],
        recent: vec!["zeta".into()],
        ..Default::default()
    }
}

#[test]
fn new_branch_then_the_current_the_recent_the_local_and_the_untracked_remote_ones() {
    let (rows, items) = list(&repo());
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels[1..], ["main", "zeta", "alpha", "origin/other"]);
    assert_eq!(rows[0], Row::NewBranch);
    assert_eq!(rows[1], Row::Branch { wanted: Wanted::Branch("main".into()), current: true });
    assert_eq!(items[1].detail, "↑2 · ↓1 · origin/main");
    assert_eq!(
        rows[4],
        Row::Branch { wanted: Wanted::Remote("origin/other".into()), current: false }
    );
}

#[test]
fn its_search_has_the_keys_and_edits_as_a_field_does() {
    let (rows, items) = list(&repo());
    let mut picker = BranchPicker::default();
    picker.set_data(&BranchList { anchor: None, rows, items });
    assert!(picker.text_input("zet"));
    assert!(picker.command("select_all"));
    assert_eq!(picker.selected_text().as_deref(), Some("zet"));
    assert!(picker.paste("al\npha"));
    assert_eq!(picker.picker.shown().len(), 0, "`al pha` finds nothing");
    assert!(picker.command("undo"));
    // Esc closes it: the window it's in is told to go (never left open unseen).
    picker.edit_key(ViewerKey::Escape);
    assert!(picker.closed());
    assert!(matches!(picker.take_action(), Some(ViewerAction::Close)));
}

#[test]
fn choosing_a_branch_switches_to_it_and_new_branch_opens_its_dialog() {
    let (rows, items) = list(&repo());
    let mut picker = BranchPicker::default();
    picker.set_data(&BranchList { anchor: None, rows, items });
    picker.picked(Some(PickerEvent::Chosen(2)));
    assert!(picker.closed());
    assert!(matches!(picker.take_action(), Some(ViewerAction::WithHost(_))));
}

/// The wheel is its own only while the mouse is over the list; elsewhere it goes on to what's
/// under the popover.
#[test]
fn it_takes_the_wheel_only_with_the_mouse_over_it() {
    let (rows, items) = list(&repo());
    let mut picker = BranchPicker::default();
    picker.set_data(&BranchList { anchor: None, rows, items });
    assert!(!picker.takes_scroll());
    picker.hover(Point::new(-100.0, -100.0));
    assert!(!picker.takes_scroll());
}

/// Many branches: the list's thumb pressed holds the mouse (nothing chosen) and dragged
/// down scrolls the rows.
#[test]
fn the_list_scrolls_by_its_thumb() {
    let mut rows = vec![Row::NewBranch];
    let mut items = vec![PickerItem { label: "New Branch…".into(), ..PickerItem::default() }];
    for i in 0..40 {
        rows.push(Row::Branch { wanted: Wanted::Branch(format!("b{i}")), current: false });
        items.push(PickerItem { label: format!("b{i}"), ..PickerItem::default() });
    }
    let mut view = BranchPicker::default();
    view.set_data(&BranchList { anchor: None, rows, items });
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut paint = |view: &mut BranchPicker| {
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 900.0, 700.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut render_api::Scene::default(),
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view);
    let list = view.picker.list_area();
    let edge = list.max_x() - 4.0;
    let thumb = Point::new(edge, list.min_y() + 6.0);
    view.hover(thumb);
    assert!(view.press(thumb, 1).is_none() && !view.closed(), "nothing chosen");
    assert!(view.picker.dragging(), "the thumb holds the mouse");
    let shown = view.picker.highlighted();
    assert!(view.drag(Point::new(edge, list.min_y() + 120.0)), "the rows follow");
    assert!(view.release());
    assert!(!view.picker.dragging());
    assert_eq!(view.picker.highlighted(), shown, "the keys' row kept");
}

/// The list has the focus from the moment it opens: Tab stays in it, the wheel over it
/// scrolls it (through its parts' controls).
#[test]
fn the_open_list_has_the_focus_and_the_wheel() {
    let mut rows = vec![Row::NewBranch];
    let mut items = vec![PickerItem { label: "New Branch…".into(), ..PickerItem::default() }];
    for i in 0..40 {
        rows.push(Row::Branch { wanted: Wanted::Branch(format!("b{i}")), current: false });
        items.push(PickerItem { label: format!("b{i}"), ..PickerItem::default() });
    }
    let mut view = BranchPicker::default();
    view.set_data(&BranchList { anchor: None, rows, items });
    assert_eq!(view.controls.focused(), Some(0), "focused as it opens");
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 900.0, 700.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(view.edit_key(ViewerKey::Tab));
    assert_eq!(view.controls.focused(), Some(0), "Tab is the list's");
    let list = view.picker.list_area();
    view.hover(Point::new(list.min_x() + 20.0, list.min_y() + 20.0));
    assert!(view.takes_scroll());
    assert!(view.scroll(0.0, 60.0), "the wheel scrolls the rows");
}
