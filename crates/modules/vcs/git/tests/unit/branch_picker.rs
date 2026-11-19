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
