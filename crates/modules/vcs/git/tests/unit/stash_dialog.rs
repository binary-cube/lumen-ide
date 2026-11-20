use super::*;
use crate::fake::FakeRepository;

fn pushing(staged: bool, paths: Vec<String>) -> StashDialog {
    let mut d = StashDialog::default();
    let asked = StashAsked::Push { branch: "main".into(), staged, preset_staged: false, paths };
    d.set_data(&StashRequest { repo: Arc::new(FakeRepository::default()), asked });
    d
}

#[test]
fn options_that_don_t_go_together_grey_each_other_out() {
    let mut d = pushing(true, Vec::new());
    d.tick(Hit::StagedOnly);
    assert!(d.staged_only);
    assert!(!d.can_tick(Hit::KeepIndex) && !d.can_tick(Hit::Untracked));
    d.tick(Hit::KeepIndex);
    assert!(!d.keep_index, "refused while only staged is ticked");
    d.tick(Hit::StagedOnly);
    d.tick(Hit::Untracked);
    assert!(!d.can_tick(Hit::StagedOnly));
    // A git without it, or chosen files: never only staged.
    assert!(!pushing(false, Vec::new()).can_tick(Hit::StagedOnly));
    assert!(!pushing(true, vec!["a.php".into()]).can_tick(Hit::StagedOnly));
}

#[test]
fn the_message_is_one_line_and_empty_means_git_s_own() {
    let mut d = pushing(true, vec!["a.php".into()]);
    assert_eq!(d.options().message, None);
    d.paste("first\nsecond");
    let options = d.options();
    assert_eq!(options.message.as_deref(), Some("first second"));
    assert_eq!(options.paths, ["a.php"]);
}

#[test]
fn applying_drops_it_after_only_when_asked() {
    let repo = Arc::new(FakeRepository::default());
    repo.stashes.lock().unwrap().push(Stash {
        name: "stash@{0}".into(),
        message: "wip".into(),
        id: "s1".into(),
    });
    let mut d = StashDialog::default();
    let asked = StashAsked::Apply(repo.stashes()[0].clone());
    d.set_data(&StashRequest { repo: Arc::clone(&repo) as Arc<dyn Repository>, asked });
    d.tick(Hit::Pop);
    assert!(d.go());
    for _ in 0..200 {
        d.poll();
        if d.closed() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(d.closed());
    assert!(repo.stashes().is_empty(), "popped");
}

/// Paints the dialog in a modal's room.
fn paint(d: &mut StashDialog) {
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    d.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 560.0, 300.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// A press in the message's field and a drag select what it covers; a drag after a press
/// on an option selects nothing.
#[test]
fn a_drag_in_the_message_selects_it() {
    let mut d = pushing(true, Vec::new());
    d.paste("a message to select");
    paint(&mut d);
    let (field, _) = *d.hits.iter().find(|(_, h)| *h == Hit::Message).unwrap();
    let mid = field.min_y() + field.size.height / 2.0;
    let (keep, _) = *d.hits.iter().find(|(_, h)| *h == Hit::KeepIndex).unwrap();
    d.press(Point::new(keep.min_x() + 4.0, keep.min_y() + 4.0), 1);
    d.drag(Point::new(field.min_x() + 4.0, mid));
    d.release();
    assert!(d.keep_index && d.selected_text().is_none(), "the option ticked, nothing selected");
    d.press(Point::new(field.max_x() - 4.0, mid), 1);
    assert!(d.drag(Point::new(field.min_x() + 4.0, mid)));
    d.release();
    assert_eq!(d.selected_text().as_deref(), Some("a message to select"));
}

/// The message's field has the keys from the start and keeps them through a press on an
/// option: the keys and the text typed after it still edit the message.
#[test]
fn the_message_keeps_the_keys_through_a_press_on_an_option() {
    let mut d = pushing(true, Vec::new());
    assert_eq!(d.controls.focused(), Some(0));
    paint(&mut d);
    let (keep, _) = *d.hits.iter().find(|(_, h)| *h == Hit::KeepIndex).unwrap();
    d.press(Point::new(keep.min_x() + 4.0, keep.min_y() + 4.0), 1);
    assert_eq!(d.controls.focused(), Some(0), "the field kept the keys");
    assert!(d.text_input("wip"));
    assert!(d.edit_key(ViewerKey::Backspace));
    assert!(!d.edit_key(ViewerKey::Tab), "one field: Tab is no move");
    assert_eq!(d.message.text(), "wi");
}
