use super::*;

#[test]
fn a_line_starts_where_it_should() {
    let text = "one\ntwo\nthree";
    assert_eq!(line_start(text, 1), 0);
    assert_eq!(line_start(text, 0), 0);
    assert_eq!(line_start(text, 2), 4);
    assert_eq!(line_start(text, 3), 8);
    assert_eq!(line_start(text, 99), text.len(), "past the end: the end");
    assert_eq!(line_start(text, u64::MAX), text.len());
}

#[test]
fn a_path_is_said_relative_to_the_project() {
    let root = std::path::Path::new("/work/p");
    assert_eq!(relative(std::path::Path::new("/work/p/src/a.php"), root), "src/a.php");
    assert_eq!(relative(std::path::Path::new("/elsewhere/b"), root), "/elsewhere/b");
}

/// Every tool says what it takes, and reads only (nothing asked of the user).
#[test]
fn the_tools_read_and_say_what_they_take() {
    let tools: [&dyn McpTool; 4] = [&OpenFile, &Selection, &OpenFiles, &FileText];
    for tool in tools {
        assert_eq!(tool.kind(), ToolKind::Read, "{}", tool.name());
        assert_eq!(tool.input_schema()["type"], "object");
        assert!(!tool.description().is_empty());
    }
}
