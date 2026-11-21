use super::*;

/// A program started under another name (its command's first word, as a program named after
/// its version is): known by that name and by its program's, its tree counted, its running
/// time measured.
#[test]
fn a_program_is_known_by_the_name_it_was_started_by() {
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new("/bin/sleep")
        .arg0("/some/folder/my-tool")
        .arg("30")
        .spawn()
        .expect("a program to look at");
    std::thread::sleep(Duration::from_millis(1100));
    let mut trees = ProcessTrees::default();
    let used = trees.sample(&[child.id()]);
    let tree = used[&child.id()];
    assert_eq!(tree.processes, 1);
    assert!(tree.running >= Duration::from_secs(1), "{:?}", tree.running);
    assert_eq!(trees.invoked(child.id()).as_deref(), Some("my-tool"));
    assert_eq!(trees.name(child.id()).as_deref(), Some("sleep"));
    // Read once, kept while it runs.
    assert_eq!(trees.invoked(child.id()).as_deref(), Some("my-tool"));
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(trees.invoked(u32::MAX - 1), None, "no such program");
}

/// The trees are walked through each process's children: each once (a cycle too), one that
/// ended has nothing under it, and one whose children can't be read while it runs means
/// every process must be looked at instead.
#[test]
fn a_tree_is_walked_through_its_children() {
    let table = |pid: u32| match pid {
        1 => Some(vec![2, 3]),
        2 => Some(vec![4]),
        3 | 4 => Some(Vec::new()),
        // A stale table: a child naming its parent.
        5 => Some(vec![6]),
        6 => Some(vec![5]),
        _ => None,
    };
    let mut found = super::trees_of(&[1], &table, &|_| true).expect("all readable");
    found.sort_unstable();
    assert_eq!(found, vec![1, 2, 3, 4]);
    let mut cycle = super::trees_of(&[5], &table, &|_| true).expect("readable");
    cycle.sort_unstable();
    assert_eq!(cycle, vec![5, 6], "each once");
    assert_eq!(super::trees_of(&[9], &table, &|_| false), Some(vec![9]), "ended: nothing under");
    assert_eq!(super::trees_of(&[9], &table, &|_| true), None, "running, unreadable: look at all");
}
