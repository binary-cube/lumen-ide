use super::*;
use remote::properties::{Mode, Principal};

fn props(mode: u32, is_dir: bool) -> Properties {
    Properties {
        path: "a".into(),
        is_dir,
        size: 0,
        modified: 0,
        mode: Some(Mode::new(mode)),
        owner: Some(Principal::named("deploy", Some(1000))),
        group: None,
    }
}

/// A row says the mode and the owner; marked when the web server can't read it.
#[test]
fn a_row_says_its_mode_and_owner_and_warns_when_unreadable() {
    let note = note_of(&props(0o644, false)).unwrap();
    assert_eq!(&*note.text, "rw-r--r-- deploy");
    assert!(!note.warn);
    assert!(note_of(&props(0o600, false)).unwrap().warn);
    // A folder: read without entering is no use to the web server.
    assert!(note_of(&props(0o744, true)).unwrap().warn);
    assert!(!note_of(&props(0o750, true)).unwrap().warn);
    // No mode said (a Windows server): nothing to show.
    let mut unknown = props(0, false);
    unknown.mode = None;
    assert_eq!(note_of(&unknown), None);
    assert!(web_readable(&unknown));
}
