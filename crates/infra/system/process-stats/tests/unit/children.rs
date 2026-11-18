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
