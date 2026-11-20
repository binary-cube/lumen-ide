use super::*;
use crate::fake::FakeRepository;

fn dialog(repo: Arc<FakeRepository>) -> SwitchDialog {
    let mut d = SwitchDialog::default();
    let request = SwitchRequest {
        repo,
        target: SwitchTarget::Branch("feature".into()),
        label: "feature".into(),
        files: vec![InTheWay { path: "a.php".into(), state: version_control::FileState::Modified }],
    };
    d.set_data(&request);
    d
}

fn settle(d: &mut SwitchDialog) {
    for _ in 0..200 {
        d.poll();
        if d.running.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn stash_and_switch_puts_the_work_aside_then_switches() {
    let repo = Arc::new(FakeRepository::default());
    let mut d = dialog(Arc::clone(&repo));
    assert!(d.go(true));
    settle(&mut d);
    assert!(d.closed());
    let pushes = repo.stash_pushes.lock().unwrap().clone();
    assert_eq!(pushes.len(), 1);
    assert!(pushes[0].untracked, "the untracked files go too");
    assert!(!repo.switched.lock().unwrap()[0].1);
    // The notification brings it back.
    let notes = d.take_notifications();
    assert_eq!(notes[0].actions[0].command, crate::POP_SWITCH_STASH);
}

#[test]
fn throwing_the_changes_away_is_asked_twice() {
    let repo = Arc::new(FakeRepository::default());
    let mut d = dialog(Arc::clone(&repo));
    d.press_hit(Hit::Discard);
    assert!(d.armed && d.running.is_none(), "armed, nothing done yet");
    d.press_hit(Hit::Discard);
    settle(&mut d);
    assert!(repo.switched.lock().unwrap().clone()[0].1, "discarding");
    assert!(repo.stash_pushes.lock().unwrap().is_empty());
}

/// Paints the dialog in a modal's room.
fn paint(d: &mut SwitchDialog) {
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    d.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 620.0, 400.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// Many files in the way: the list's thumb pressed and dragged down scrolls them.
#[test]
fn the_files_thumb_is_dragged() {
    let mut d = SwitchDialog::default();
    let files = (0..30)
        .map(|i| InTheWay {
            path: format!("f{i}.php"),
            state: version_control::FileState::Modified,
        })
        .collect();
    d.set_data(&SwitchRequest {
        repo: Arc::new(FakeRepository::default()),
        target: SwitchTarget::Branch("feature".into()),
        label: "feature".into(),
        files,
    });
    paint(&mut d);
    // A hidden bar shows once the list scrolls.
    assert!(Viewer::scroll(&mut d, 0.0, 1.0));
    paint(&mut d);
    let lane = d.list;
    let y = lane.min_y() + 6.0;
    let x = (0..16)
        .map(|dx| lane.max_x() - 1.0 - dx as f32)
        .find(|x| {
            Viewer::press(&mut d, Point::new(*x, y), 1);
            d.bar.is_dragging()
        })
        .expect("the thumb at the list's top");
    let before = d.scroll;
    assert!(d.drag(Point::new(x, y + 60.0)));
    assert!(d.scroll > before, "{} > {before}", d.scroll);
    assert!(d.release());
    assert!(!d.bar.is_dragging());
}

/// The wheel anywhere over the dialog scrolls the files, never past their end.
#[test]
fn the_wheel_over_the_dialog_scrolls_the_files() {
    let mut d = dialog(Arc::new(FakeRepository::default()));
    let files = (0..30)
        .map(|i| InTheWay {
            path: format!("f{i}.php"),
            state: version_control::FileState::Modified,
        })
        .collect();
    if let Some(request) = d.request.as_mut() {
        request.files = files;
    }
    paint(&mut d);
    // Over the note above the files, not over them.
    d.hover(Point::new(30.0, 10.0));
    assert!(Viewer::scroll(&mut d, 0.0, 10_000.0));
    assert_eq!(d.scroll, d.max_scroll, "at their end");
    assert!(!Viewer::scroll(&mut d, 0.0, 10.0), "nothing further");
}
