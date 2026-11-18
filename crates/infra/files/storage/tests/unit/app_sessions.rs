//! The sessions written and read back as they were; a damaged entry left out.

use super::*;

#[test]
fn sessions_are_written_and_read_back() {
    let dir = std::env::temp_dir().join(format!("sessions-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sessions.toml");
    let mut sessions = Sessions::default();
    sessions.record(
        7,
        100,
        vec![
            SessionWindow {
                project: Some(PathBuf::from("/work/shop")),
                files: vec![PathBuf::from("/work/shop/a.php")],
            },
            SessionWindow { project: None, files: vec![PathBuf::from("/tmp/notes.md")] },
        ],
    );
    sessions.end(7);
    save(&path, &sessions).unwrap();
    assert_eq!(load(&path), sessions);
    std::fs::write(&path, "[[sessions]]\nid = \"zz\"\n[[sessions]]\nid = \"0a\"\n").unwrap();
    let read = load(&path);
    assert_eq!(read.list.len(), 1, "the damaged one left out");
    assert_eq!(read.list[0].id, 10);
    assert_eq!(load(&dir.join("none.toml")), Sessions::default());
    let _ = std::fs::remove_dir_all(dir);
}
