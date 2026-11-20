use super::*;

/// The message's edits are kept as the repository's draft; Tab is kept for the screen.
#[test]
fn the_message_keeps_its_draft_at_every_edit() {
    let root = std::path::PathBuf::from("/draft-of-the-message-part");
    let mut field = MultilineField::default();
    let mut message = MessagePart { field: &mut field, draft: Some(&root) };
    assert!(message.keeps_tab() && message.key(ViewerKey::Tab, KeyReach::default()));
    assert!(message.text("fix"));
    assert_eq!(super::super::drafts().get(&root).map(String::as_str), Some("fix"));
    assert!(message.key(ViewerKey::Backspace, KeyReach::default()));
    assert!(message.command("undo"));
    assert_eq!(super::super::drafts().get(&root).map(String::as_str), Some("fix"));
    assert!(!message.key(ViewerKey::Escape, KeyReach::default()));
    super::super::drafts().remove(&root);
}

/// The places' filter: edits noted, Esc empties it, then asks to leave it (as Tab does).
#[test]
fn the_places_filter_edits_then_asks_to_leave() {
    let mut filter = TextInput::new("");
    let (mut edited, mut leave) = (false, false);
    let mut lane = PlacesLane { filter: &mut filter, edited: &mut edited, leave: &mut leave };
    assert!(lane.text("ven"));
    assert!(lane.key(ViewerKey::Escape, KeyReach::default()));
    assert!(TextInput::text(lane.filter).is_empty());
    assert!(lane.key(ViewerKey::Escape, KeyReach::default()));
    assert!(edited && leave);
}

/// The lists take their moving keys (written for the screen to do) and all that's typed;
/// not the field's keys.
#[test]
fn the_lists_answer_their_keys_and_the_text() {
    let mut asked = None;
    let mut lane = ListsLane { asked: &mut asked };
    let shift = KeyReach { extend: true, ..KeyReach::default() };
    assert!(lane.key(ViewerKey::Down, shift));
    assert!(!lane.key(ViewerKey::Left, KeyReach::default()));
    assert_eq!(asked, Some(ListsAsked::Key(ViewerKey::Down, shift)));
    let mut lane = ListsLane { asked: &mut asked };
    assert!(lane.text("F"));
    assert_eq!(asked, Some(ListsAsked::Text("F".into())));
}
