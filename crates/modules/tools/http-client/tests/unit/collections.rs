//! The project's requests as a tree: folders at any depth (an empty one too), their
//! requests (the file isn't a level), collapsed and searched; names typed; where a name is
//! typed among the rows; the collections' folder walked.

use super::*;

fn listing() -> Listing {
    let texts = [
        (
            "http/users/users.http",
            "# @name login\nPOST https://e.com/login\n\n###\nGET https://e.com/me\n",
        ),
        ("http/users/admin/admin.http", "GET https://e.com/admin\n"),
        ("api/orders.rest", "GET https://e.com/orders\n"),
        ("root.http", "DELETE https://e.com/x\n"),
        ("notes.txt", "GET https://e.com/not-listed\n"),
    ];
    let list: Vec<String> = texts.iter().map(|(p, _)| (*p).to_owned()).collect();
    let files = read(&list, &["http", "rest"], &|path| {
        texts.iter().find(|(p, _)| Path::new(p) == path).map(|(_, t)| (*t).to_owned())
    });
    // An empty collection: only its folder.
    Listing { files, folders: vec!["http".into(), "http/empty".into(), "http/users".into()] }
}

fn shown(rows: &[Row], listing: &Listing) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            Row::Folder { path, depth, .. } => format!("{depth}:{}/", base_name(path)),
            Row::Request { file, request, depth } => {
                format!("{depth}:{}", listing.files[*file].requests[*request].label)
            }
        })
        .collect()
}

#[test]
fn files_are_read_by_path_with_their_requests() {
    let listing = listing();
    let paths: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["root.http", "api/orders.rest", "http/users/users.http", "http/users/admin/admin.http"],
        "not the .txt; by folder, then name"
    );
    let login = &listing.files[2].requests[0];
    assert_eq!((login.label.as_str(), login.method.as_str(), login.line), ("login", "POST", 1));
    assert_ne!(login.hash, listing.files[2].requests[1].hash, "each block its own fingerprint");
}

#[test]
fn the_rows_are_a_tree_of_folders_and_their_requests() {
    let listing = listing();
    let all = rows(&listing, &BTreeSet::new(), "", "");
    assert_eq!(
        shown(&all, &listing),
        [
            "0:api/",
            "1:GET https://e.com/orders",
            "0:http/",
            "1:empty/",
            "1:users/",
            "2:admin/",
            "3:GET https://e.com/admin",
            "2:login",
            "2:GET https://e.com/me",
            "0:DELETE https://e.com/x",
        ],
        "folders first, then requests; no file level; an empty collection shown"
    );
    // A collection's own tree: what's in it, from depth 0.
    let users = rows(&listing, &BTreeSet::new(), "", "http/users");
    assert_eq!(
        shown(&users, &listing),
        ["0:admin/", "1:GET https://e.com/admin", "0:login", "0:GET https://e.com/me"]
    );
}

#[test]
fn rows_follow_what_s_collapsed_and_what_s_searched() {
    let listing = listing();
    let closed: BTreeSet<String> = [collapse_key("http/users")].into();
    let shut = rows(&listing, &closed, "", "http");
    assert_eq!(shut.len(), 2, "empty/, users/ closed");
    assert_eq!(shut[1], Row::Folder { path: "http/users".into(), depth: 0, expanded: false });
    // A search opens what holds a match and shows only that.
    let found = rows(&listing, &closed, "LOGIN", "");
    assert_eq!(shown(&found, &listing), ["0:http/", "1:users/", "2:login"]);
    let admin = shown(&rows(&listing, &closed, "admin", ""), &listing);
    assert_eq!(admin, ["0:http/", "1:users/", "2:admin/", "3:GET https://e.com/admin"]);
    // A folder found: all it holds.
    let users = shown(&rows(&listing, &BTreeSet::new(), "users", ""), &listing);
    assert_eq!(users.len(), 6, "{users:?}");
}

#[test]
fn a_collection_is_its_folder_under_the_collections_one() {
    assert_eq!(collection_of("http/users/admin"), "http/users");
    assert_eq!(collection_of("http/users"), "http/users");
    assert_eq!(collection_of("api"), "api");
    assert_eq!((parent("a/b/c"), parent("a"), base_name("a/b")), ("a/b", "", "b"));
}

#[test]
fn the_collections_folder_is_walked_its_links_aside() {
    let root = std::env::temp_dir().join(format!("http-walk-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(root.join("http/a/b")).unwrap();
    std::fs::create_dir_all(root.join("http/.hidden")).unwrap();
    std::fs::write(root.join("http/a/x.http"), "GET https://e.com\n").unwrap();
    std::fs::write(root.join("http/a/notes.md"), "").unwrap();
    let (folders, files) = walk(&root, COLLECTIONS);
    assert_eq!(folders, ["http", "http/a", "http/a/b"]);
    assert_eq!(files, ["http/a/x.http"]);
    assert_eq!(walk(&root, "missing"), (Vec::new(), Vec::new()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_new_file_gets_a_free_name() {
    let taken = |name: &str| ["requests.http", "requests-2.http"].contains(&name);
    assert_eq!(free_name("requests", "", &taken), "requests-3.http");
    assert_eq!(free_name("requests", "http", &taken), "http/requests.http");
}

#[test]
fn names_typed_are_one_part_of_a_path() {
    assert_eq!(valid_name("  Users API "), Some("Users API".into()));
    for bad in ["", " ", ".", "..", "a/b", "a\\b", "../x", "c:d", "a?", "x\ny"] {
        assert_eq!(valid_name(bad), None, "{bad:?}");
    }
}

#[test]
fn a_name_is_typed_where_its_entry_goes() {
    let listing = listing();
    let rows = rows(&listing, &BTreeSet::new(), "", "");
    let plain: Vec<Slot> = (0..rows.len()).map(Slot::Row).collect();
    assert_eq!(slots(&rows, None), plain);
    assert_eq!(slots(&rows, Some(NameFor::Top))[0], Slot::Naming(0));
    let users = rows
        .iter()
        .position(|r| matches!(r, Row::Folder { path, .. } if path == "http/users"))
        .unwrap();
    let inside = slots(&rows, Some(NameFor::In("http/users")));
    assert_eq!((inside[users], inside[users + 1]), (Slot::Row(users), Slot::Naming(2)));
    let renamed = slots(&rows, Some(NameFor::Folder("http/users")));
    assert_eq!((renamed[users], renamed.len()), (Slot::Naming(1), rows.len()), "on its row");
}
