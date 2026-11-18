use super::*;

/// A statement's hash is its text's: the same text the same hash, any change another.
#[test]
fn a_statement_s_hash_follows_its_text() {
    assert_eq!(text_hash("SELECT 1"), text_hash("SELECT 1"));
    assert_ne!(text_hash("SELECT 1"), text_hash("SELECT 2"));
    assert_ne!(text_hash(""), text_hash(" "));
}

struct Plain;

impl EditorToolbar for Plain {
    fn paint(&mut self, _ctx: ToolbarPaint<'_>) {}
}

/// A bar that only draws: the file's title, icon and breadcrumb, nothing marked, it closes.
#[test]
fn a_bar_s_defaults_change_nothing_of_the_file_s() {
    let mut bar = Plain;
    assert_eq!(bar.height(), HEIGHT);
    assert!(bar.title().is_none() && bar.icon().is_none() && bar.crumbs().is_empty());
    assert!(bar.may_close());
    assert!(bar.marks().0.is_empty() && bar.highlight().is_none());
    assert!(!bar.follows_editor() && !bar.command("x") && bar.take_action().is_none());
}
