use super::*;

/// Opened on a folder with nothing kept: measured, shown, kept; opened again: what's kept
/// shown at once. A folder clicked is gone into; the path above goes back.
#[test]
fn a_folder_is_measured_kept_and_gone_into() {
    let root = crate::scan::tests::scratch("view");
    let storage = root.join(".data");
    std::fs::create_dir_all(root.join("big")).unwrap();
    std::fs::write(root.join("big/a.bin"), vec![0u8; 4000]).unwrap();
    std::fs::write(root.join("b.txt"), vec![0u8; 10]).unwrap();
    let data = SizesData { folder: root.clone(), storage: Some(storage.clone()) };
    let mut view = SizesView::default();
    view.set_data(&data);
    wait(&mut view);
    let measured = view.report().cloned().expect("measured");
    assert!(measured.root.size >= 4010);
    assert!(report::load(&storage, &root).is_some(), "kept");
    let big = view.node().unwrap().children.iter().position(|c| c.name == "big").unwrap();
    assert!(view.enter(big));
    assert_eq!(view.crumb_names().last().map(String::as_str), Some("big"));
    assert!(view.edit_key(ViewerKey::Backspace));
    assert!(view.trail.is_empty());
    let mut again = SizesView::default();
    again.set_data(&data);
    wait(&mut again);
    assert_eq!(again.report().map(|r| r.at), Some(measured.at), "the kept one");
    let _ = std::fs::remove_dir_all(root);
}

fn wait(view: &mut SizesView) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while (view.loading.is_some() || view.measuring.is_some()) && Instant::now() < deadline {
        view.take();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
