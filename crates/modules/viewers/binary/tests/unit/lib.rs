use super::*;

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("binary-files-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn only_files_that_arent_text_are_claimed() {
    let video = temp("clip.mp4", b"\0\0\0\x18ftypmp42\0\0\0\0");
    let php = temp("a.php", b"<?php echo 1;\n");
    assert_eq!(Provider.kind(&video), Some(ViewerKind::Replace));
    assert_eq!(Provider.kind(&php), None);
    // Virtual tabs (no file) aren't.
    assert_eq!(Provider.kind(Path::new("/nowhere/x.view-diff")), None);
}

#[test]
fn reveal_asks_the_app() {
    let mut view = BinaryView::new("/p/clip.mp4".into(), TextCheck::Binary, 3 << 30);
    view.reveal = Rect::new(10.0, 10.0, 100.0, 30.0);
    assert!(matches!(
        view.click(Point::new(20.0, 20.0)),
        Some(ViewerAction::Request(AppRequest::Reveal(p))) if p == Path::new("/p/clip.mp4")
    ));
    assert_eq!(view.click(Point::new(0.0, 0.0)), None);
    assert_eq!(view.status(), Some(i18n::bytes(3 * 1024 * 1024 * 1024)));
}
