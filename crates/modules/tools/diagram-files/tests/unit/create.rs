use super::*;

/// New diagrams under free names, opened as diagrams; folders too; the project folder's
/// `.gitignore` letting them through (another module's line kept).
#[test]
fn new_diagrams_and_folders() {
    let root = std::env::temp_dir()
        .join(format!("diagrams-create-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let own = root.join(kernel::brand::PROJECT_DIR);
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(own.join(".gitignore"), "*\n!path-marks\n").unwrap();
    let dir = folder(&root);
    let first = new_diagram(&root, &dir).unwrap();
    let second = new_diagram(&root, &dir).unwrap();
    assert_eq!(first.file_name().unwrap(), "Untitled.drawio");
    assert_eq!(second.file_name().unwrap(), "Untitled 2.drawio");
    let doc = diagram_model::Document::read(&std::fs::read_to_string(&first).unwrap()).unwrap();
    assert_eq!(doc.pages.len(), 1);
    let sub = new_folder(&root, &dir).unwrap();
    assert!(sub.is_dir() && sub.file_name().unwrap() == "New folder");
    assert_eq!(
        std::fs::read_to_string(own.join(".gitignore")).unwrap(),
        "*\n!path-marks\n!diagrams/\n!diagrams/**\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
