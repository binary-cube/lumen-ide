use super::*;

#[test]
fn primary_modifier_matches_platform_convention() {
    let cmd = Modifiers { meta: true, ..Modifiers::default() };
    let ctrl = Modifiers { ctrl: true, ..Modifiers::default() };
    if cfg!(target_os = "macos") {
        assert!(cmd.primary() && !ctrl.primary());
    } else {
        assert!(ctrl.primary() && !cmd.primary());
    }
}
