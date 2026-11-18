use super::*;

#[test]
fn a_file_is_read_whole_and_written_whole() {
    let dir = std::env::temp_dir().join(format!("diagram-web-file-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.drawio");
    // Not there: told apart from an empty file.
    assert_eq!(read(&path).unwrap(), None);
    write(&path, "<mxfile>ă</mxfile>").unwrap();
    assert_eq!(read(&path).unwrap().as_deref(), Some("<mxfile>ă</mxfile>"));
    // Not text: refused, never mangled.
    std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
    assert!(read(&path).is_err());
    // No temporary file left beside it.
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A write never goes over someone else's change: the file stays as they left it, what was
/// to be written is kept beside it; unchanged since read (or our own text there), written.
#[test]
fn a_write_never_goes_over_someone_else_s_change() {
    let dir = std::env::temp_dir().join(format!("diagram-web-over-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flow.drawio");
    write(&path, "read").unwrap();
    assert_eq!(write_over(&path, "mine", Some("read")).unwrap(), Wrote::Done);
    assert_eq!(read(&path).unwrap().as_deref(), Some("mine"));
    // Someone else wrote meanwhile ("theirs"): left as it is.
    write(&path, "theirs").unwrap();
    let Wrote::Conflict { disk, kept } = write_over(&path, "newer", Some("mine")).unwrap() else {
        panic!("a conflict")
    };
    assert_eq!((&*disk, read(&path).unwrap().as_deref()), ("theirs", Some("theirs")));
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "newer");
    assert_eq!(kept.parent(), Some(dir.as_path()), "beside it");
    assert!(kept.extension().is_some_and(|e| e == "drawio"));
    // A new file (none read): written.
    let new = dir.join("new.drawio");
    assert_eq!(write_over(&new, "first", None).unwrap(), Wrote::Done);
    std::fs::remove_dir_all(dir).unwrap();
}
