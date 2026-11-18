use super::*;

/// Kept and read back; another folder's, or a broken file, isn't taken.
#[test]
fn a_report_is_kept_and_read_back() {
    let storage = std::env::temp_dir()
        .join(format!("folder-sizes-report-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let folder = PathBuf::from("/work/project/vendor");
    let report = Report {
        path: folder.clone(),
        at: 1_700_000_000,
        took_ms: 12,
        partial: false,
        unreadable: 1,
        root: Node { name: "vendor".into(), dir: true, size: 42, files: 3, ..Node::default() },
    };
    save(&storage, &report).unwrap();
    assert_eq!(load(&storage, &folder), Some(report));
    assert_eq!(load(&storage, Path::new("/work/project/other")), None);
    std::fs::write(file(&storage, &folder), "{ broken").unwrap();
    assert_eq!(load(&storage, &folder), None);
    let _ = std::fs::remove_dir_all(storage);
}
