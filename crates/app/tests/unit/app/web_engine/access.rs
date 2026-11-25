//! A page's accessibility nodes as the window's.

use super::*;

/// Chromium's roles as the window's tree has them: what has none is a group; a field with
/// several lines a text area; the states kept.
#[test]
fn page_nodes_take_the_windows_roles() {
    use platform_api::access::Role;
    let node = |role: &str, value: Option<&str>| web_engine::protocol::AccessNode {
        id: 3,
        role: role.into(),
        value: value.map(Into::into),
        rect: [1.0, 2.0, 3.0, 4.0],
        focused: true,
        checked: Some(false),
        ..Default::default()
    };
    let told = access_node(node("checkBox", None));
    assert_eq!(told.role, Role::CheckBox);
    assert!(told.state.focused);
    assert_eq!(told.state.checked, Some(false));
    assert_eq!(told.bounds, kernel::Rect::new(1.0, 2.0, 3.0, 4.0));
    assert_eq!(access_node(node("textField", Some("a\nb"))).role, Role::TextArea);
    assert_eq!(access_node(node("textField", Some("a"))).role, Role::TextField);
    assert_eq!(access_node(node("genericContainer", None)).role, Role::Group);
}
