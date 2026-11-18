use super::*;

fn scratch() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "let-through-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Made when missing (everything else ignored); another module's lines kept; nothing
/// written twice.
#[test]
fn each_module_adds_only_its_lines() {
    let dir = scratch();
    let read = || std::fs::read_to_string(dir.join(".gitignore")).unwrap();
    let_through(&dir, &["!path-marks"]).unwrap();
    assert_eq!(read(), "*\n!path-marks\n");
    let_through(&dir, &["!diagrams/", "!diagrams/**"]).unwrap();
    assert_eq!(read(), "*\n!path-marks\n!diagrams/\n!diagrams/**\n");
    let stamp = std::fs::metadata(dir.join(".gitignore")).unwrap().modified().unwrap();
    let_through(&dir, &["!diagrams/"]).unwrap();
    assert_eq!(
        std::fs::metadata(dir.join(".gitignore")).unwrap().modified().unwrap(),
        stamp,
        "not written again"
    );
    // A file without its last line break: the new line still on its own.
    std::fs::write(dir.join(".gitignore"), "*").unwrap();
    let_through(&dir, &["!x"]).unwrap();
    assert_eq!(read(), "*\n!x\n");
    let _ = std::fs::remove_dir_all(&dir);
}
