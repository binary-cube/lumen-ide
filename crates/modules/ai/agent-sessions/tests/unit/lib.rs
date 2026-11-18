use super::*;

/// It offers its window on the left, under the project's; its button only while an agent is.
#[test]
fn its_window_is_on_the_left_while_an_agent_is_offered() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(SessionsModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!((tools[0].id(), tools[0].dock()), (TOOL, Dock::Left));
    assert!(tools[0].order() > 100, "after the project's");
    assert_eq!(tools[0].content(), ToolContent::Panel);
}

/// New sessions' tabs are named apart.
#[test]
fn new_sessions_are_named_apart() {
    assert_eq!(next_title("Agent X"), "Agent X");
    assert_eq!(next_title("Agent X"), "Agent X (2)");
    assert_eq!(next_title("Agent Y"), "Agent Y");
}

/// A title typed into a terminal is one line, nothing in it acting as a key.
#[test]
fn a_title_is_one_line() {
    assert_eq!(one_line("  matching system  "), "matching system");
    assert_eq!(one_line("a\rb\nc\x1b[2J"), "abc[2J");
    assert_eq!(one_line(&"x".repeat(300)).len(), 100);
    assert_eq!(one_line("\n\t"), "");
}
