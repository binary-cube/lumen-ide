use super::*;

/// A key is never a path: separators, drives, dots and spaces become dashes; length bounded.
#[test]
fn a_tab_s_key_is_safe() {
    assert_eq!(tab_key("claude-1b2c_3"), "claude-1b2c_3");
    assert_eq!(tab_key("fix src/Auth/Login.php"), "fix-src-Auth-Login-php");
    assert_eq!(tab_key("/etc/passwd"), "-etc-passwd");
    assert_eq!(tab_key("C:\\x"), "C--x");
    assert_eq!(tab_key("../.."), "-----");
    assert_eq!(tab_key(""), "-");
    assert_eq!(tab_key(&"a".repeat(200)).len(), 80);
}
