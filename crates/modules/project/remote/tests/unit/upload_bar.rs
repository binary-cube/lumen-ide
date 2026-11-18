use super::*;

/// Not a remote project (no session): no bar, no height.
#[test]
fn without_a_session_there_is_no_bar() {
    let root = PathBuf::from("/p");
    let mut bar = UploadBar::new(ProjectId::new(), root, "a.php".into());
    assert!(!bar.poll());
    assert_eq!(bar.height(), 0.0);
    assert_eq!(relative(Path::new("/p"), Path::new("/p/src/a.php")).as_deref(), Some("src/a.php"));
    assert_eq!(relative(Path::new("/p"), Path::new("/q/a.php")), None);
}

/// Its file going: Upload eases toward its spinner, then holds; done: it eases back.
#[test]
fn upload_makes_room_for_its_spinner_and_gives_it_back_eased() {
    let mut bar = UploadBar::new(ProjectId::new(), PathBuf::from("/p"), "a.php".into());
    assert!(!bar.is_animating(), "still at first");
    assert!(bar.follow_sending(true));
    assert!(bar.is_loading() && bar.is_animating(), "widening");
    assert!(bar.busy.value() < 1.0);
    assert!(!bar.follow_sending(true), "nothing new");
    std::thread::sleep(ds::BUSY_TRANSITION + std::time::Duration::from_millis(20));
    assert!(!bar.is_animating(), "it holds");
    assert!((bar.busy.value() - 1.0).abs() < f32::EPSILON);
    assert!(bar.follow_sending(false));
    assert!(!bar.is_loading() && bar.is_animating(), "narrowing back");
    assert_eq!(bar.busy.target(), 0.0);
}
