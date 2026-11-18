use super::*;

/// A folder of its own (removed at the end).
pub(crate) fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("folder-sizes-{name}-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Sizes added up folder by folder, biggest first; a link counts as itself (not followed).
#[test]
fn sizes_add_up_folder_by_folder() {
    let root = scratch("sum");
    std::fs::create_dir_all(root.join("big/inner")).unwrap();
    std::fs::create_dir_all(root.join("small")).unwrap();
    std::fs::write(root.join("big/a.bin"), vec![0u8; 3000]).unwrap();
    std::fs::write(root.join("big/inner/b.bin"), vec![0u8; 2000]).unwrap();
    std::fs::write(root.join("small/c.txt"), vec![0u8; 100]).unwrap();
    std::fs::write(root.join("top.txt"), vec![0u8; 10]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("big"), root.join("link")).unwrap();
    let progress = Progress::default();
    let node = measure(&root, &progress).unwrap();
    // The link counts as itself (its own length) where there are links.
    #[cfg(unix)]
    let links = link_len(&root);
    #[cfg(not(unix))]
    let links = 0;
    assert_eq!(node.size, 5110 + links);
    assert_eq!(node.children[0].name, "big");
    assert_eq!(node.children[0].size, 5000);
    assert_eq!(node.children[0].files, 2);
    assert_eq!(node.children[0].children[0].name, "a.bin", "biggest first");
    let small = node.children.iter().find(|c| c.name == "small").unwrap();
    assert_eq!((small.size, small.dir), (100, true));
    assert_eq!(progress.files.load(Ordering::Relaxed), node.files);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
fn link_len(root: &Path) -> u64 {
    std::fs::symlink_metadata(root.join("link")).map_or(0, |m| m.len())
}

/// Past the count listed, the smallest entries summed as one; asked to stop, it stops.
#[test]
fn the_smallest_are_summed_and_a_stop_is_heard() {
    let root = scratch("many");
    for i in 0..(FILES_MOST + 5) {
        std::fs::write(root.join(format!("f{i:03}")), vec![0u8; i + 1]).unwrap();
    }
    let node = measure(&root, &Progress::default()).unwrap();
    assert_eq!(node.children.len(), FILES_MOST + 1);
    let other = node.children.iter().find(|c| c.other).unwrap();
    assert_eq!((other.files, other.name.as_str()), (5, "5"));
    assert_eq!(node.files, (FILES_MOST + 5) as u64);
    let stopped = Progress::default();
    stopped.stop.store(true, Ordering::Relaxed);
    assert_eq!(measure(&root, &stopped).unwrap().files, 0);
    assert!(measure(&root.join("missing"), &Progress::default()).is_none());
    let _ = std::fs::remove_dir_all(root);
}
