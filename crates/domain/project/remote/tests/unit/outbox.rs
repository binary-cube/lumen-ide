use super::*;

fn sent(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn renames(o: &Outbox) -> Vec<(String, String)> {
    o.renamed().map(|(h, t)| (h.to_owned(), t.to_owned())).collect()
}

#[test]
fn a_file_moved_twice_is_one_move_and_moved_back_is_none() {
    let mut o = Outbox::default();
    o.moved("a.php", "b.php", &sent(&["a.php"]));
    o.moved("b.php", "c.php", &sent(&[]));
    assert_eq!(renames(&o), [("c.php".to_owned(), "a.php".to_owned())]);
    o.moved("c.php", "a.php", &sent(&[]));
    assert!(o.is_empty());
}

#[test]
fn a_file_never_sent_is_not_kept() {
    let mut o = Outbox::default();
    o.moved("new.php", "other.php", &sent(&[]));
    o.removed("other.php", &sent(&[]));
    assert!(o.is_empty());
}

#[test]
fn a_folder_moved_keeps_each_sent_file() {
    let mut o = Outbox::default();
    o.moved("src", "lib", &sent(&["src/a.php", "src/x/b.php"]));
    assert_eq!(
        renames(&o),
        [
            ("lib/a.php".to_owned(), "src/a.php".to_owned()),
            ("lib/x/b.php".to_owned(), "src/x/b.php".to_owned()),
        ]
    );
    // A file moved out of the moved folder keeps its server name.
    o.moved("lib/a.php", "a.php", &sent(&[]));
    assert_eq!(o.server_name("a.php"), Some("src/a.php"));
}

#[test]
fn a_moved_file_deleted_deletes_the_servers_one() {
    let mut o = Outbox::default();
    o.moved("a.php", "b.php", &sent(&["a.php"]));
    o.removed("b.php", &sent(&[]));
    assert_eq!(o.deleted().collect::<Vec<_>>(), ["a.php"]);
    assert_eq!(renames(&o), []);
    // Made anew where it was: an edit, not a deletion.
    assert!(o.revived("a.php"));
    assert!(o.is_empty());
}

#[test]
fn a_folder_deleted_deletes_its_sent_files_once() {
    let mut o = Outbox::default();
    o.moved("src/a.php", "a.php", &sent(&["src/a.php"]));
    o.removed("src", &sent(&["src/a.php", "src/b.php"]));
    // The moved file is out of the folder: still a move.
    assert_eq!(o.deleted().collect::<Vec<_>>(), ["src/b.php"]);
    assert_eq!(o.server_name("a.php"), Some("src/a.php"));
}

#[test]
fn what_was_done_on_the_server_leaves_the_list() {
    let mut o = Outbox::default();
    o.moved("src", "lib", &sent(&["src/a.php", "src/b.php"]));
    o.removed("old.php", &sent(&["old.php"]));
    o.rename_done("lib");
    o.delete_done("old.php");
    assert!(o.is_empty());
}

#[test]
fn the_outbox_round_trips_as_text() {
    let mut o = Outbox::default();
    o.moved("a b.php", "c.php", &sent(&["a b.php"]));
    o.removed("d.php", &sent(&["d.php"]));
    assert_eq!(Outbox::from_text(&o.to_text()), o);
    assert!(Outbox::from_text("X\tbroken\nR\tonly-one\n").is_empty());
}
