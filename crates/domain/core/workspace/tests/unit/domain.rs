use super::*;

fn tree() -> FileTree {
    let mut t = FileTree::new(PathBuf::from("/work/shop"));
    t.set_children(
        t.root(),
        vec![
            DirEntry::new("composer.json", NodeKind::File),
            DirEntry::new("app", NodeKind::Directory),
            DirEntry::new(".git", NodeKind::Directory),
            DirEntry::new("README.md", NodeKind::File),
            DirEntry::new("Config", NodeKind::Directory),
        ],
    );
    t
}

fn names(t: &mut FileTree) -> Vec<String> {
    let rows = t.visible_rows().to_vec();
    rows.iter().map(|r| format!("{}{}", "  ".repeat(r.depth as usize), t.name(r.id))).collect()
}

#[test]
fn directories_first_then_case_insensitive_and_git_hidden() {
    let mut t = tree();
    assert_eq!(names(&mut t), ["shop", "  app", "  Config", "  composer.json", "  README.md"]);
}

#[test]
fn expanding_shows_children_and_collapsing_hides_them() {
    let mut t = tree();
    let app = t.visible_rows()[1].id;
    assert!(t.needs_loading(app));
    t.set_children(app, vec![DirEntry::new("User.php", NodeKind::File)]);
    t.set_expanded(app, true);
    assert_eq!(names(&mut t)[1..3], ["  app", "    User.php"]);

    t.set_expanded(app, false);
    assert!(!names(&mut t).contains(&"    User.php".to_owned()));
}

#[test]
fn path_joins_ancestors() {
    let mut t = tree();
    let app = t.visible_rows()[1].id;
    t.set_children(app, vec![DirEntry::new("User.php", NodeKind::File)]);
    t.set_expanded(app, true);
    let user = t.visible_rows()[2].id;
    assert_eq!(t.path(user), PathBuf::from("/work/shop/app/User.php"));
    assert_eq!(t.path(t.root()), PathBuf::from("/work/shop"));
}

/// An entry gone on a re-read lets go of its place, with all under it; a new entry takes it,
/// under a new generation: the arena doesn't grow with entries coming and going, and an id
/// kept from before is no longer the tree's (it reads as nothing, its path is empty — never
/// the new entry's).
#[test]
fn an_entry_gone_frees_its_place_and_its_old_id_is_no_longer_the_tree_s() {
    let mut t = tree();
    let app = t.visible_rows()[1].id;
    t.set_children(app, vec![DirEntry::new("Old", NodeKind::Directory)]);
    let old = t.child_named(app, "Old").unwrap();
    t.set_children(old, vec![DirEntry::new("a.php", NodeKind::File)]);
    let inside = t.child_named(old, "a.php").unwrap();
    let size = t.nodes.len();
    t.set_children(app, vec![DirEntry::new("New", NodeKind::Directory)]);
    let new = t.child_named(app, "New").unwrap();
    assert!(!t.contains(old) && !t.contains(inside), "gone with what's under it");
    assert!(t.contains(new));
    assert_eq!(t.nodes.len(), size, "its place taken again, the arena not grown");
    assert_ne!(new, old, "the same place, another generation");
    assert_eq!(t.name(old), "");
    assert_eq!(t.path(old), PathBuf::new());
    assert_eq!(t.path(new), PathBuf::from("/work/shop/app/New"));
    // Coming and going many times: the arena stays the size of what's there.
    for i in 0..1000 {
        t.set_children(app, vec![DirEntry::new(format!("f{i}"), NodeKind::File)]);
    }
    assert!(t.nodes.len() <= size + 1, "{}", t.nodes.len());
}

/// The folders already read: the root and those opened, not those never read.
#[test]
fn the_folders_read_are_listed() {
    let mut t = tree();
    let app = t.find(Path::new("/work/shop/app")).unwrap();
    t.set_children(app, vec![DirEntry::new("Models", NodeKind::Directory)]);
    assert_eq!(t.loaded_dirs(), [PathBuf::from("/work/shop"), PathBuf::from("/work/shop/app")]);
}
