//! The private environment files added to a `.gitignore` once.

use super::*;

#[test]
fn the_private_files_are_added_once() {
    let added = with_ignored("target/\n").unwrap();
    assert!(added.starts_with("target/\n"));
    assert!(
        added.contains("\nhttp-client.private.env.json\n")
            && added.contains("\nhttp-client.local.env.json\n")
    );
    assert_eq!(with_ignored(&added), None, "already there");
    assert!(
        with_ignored("node_modules").unwrap().starts_with("node_modules\n# "),
        "a line ended first"
    );
    // Written another way: still ignored.
    let other = "/http-client.private.env.json\n**/http-client.local.env.json\n";
    assert_eq!(with_ignored(other), None);
}
