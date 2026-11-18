use super::*;

#[test]
fn each_state_is_said_as_a_word() {
    assert_eq!(state(FileState::Modified), "modified");
    assert_eq!(state(FileState::Untracked), "untracked");
    assert_eq!(state(FileState::Conflicted), "conflicted");
}

/// Both read only, each saying what it takes.
#[test]
fn the_tools_read_and_say_what_they_take() {
    let tools: [&dyn McpTool; 2] = [&GitStatus, &GitHeadText];
    for tool in tools {
        assert_eq!(tool.kind(), ToolKind::Read);
        assert_eq!(tool.input_schema()["type"], "object");
    }
    assert_eq!(GitHeadText.input_schema()["required"][0], "path");
}
