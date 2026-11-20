use super::*;
use crate::fake::FakeRepository;
use version_control::Remote;

fn dialog() -> PushDialog {
    let mut d = PushDialog::default();
    d.set_data(&PushRequest {
        repo: Arc::new(FakeRepository::default()),
        remotes: vec![Remote { name: "origin".into(), url: "git@host:a.git".into() }],
        branches: vec!["main".into()],
        states: Vec::new(),
        branch: Some("main".into()),
        remote: None,
    });
    d
}

/// Its button sends it and closes; no key does (a push is the user's to choose).
#[test]
fn its_button_sends_it_and_no_key_does() {
    let mut d = dialog();
    assert!(!d.edit_key(ViewerKey::Enter), "Enter pushes nothing");
    assert!(!d.closed() && d.take_action().is_none());
    assert!(d.command(GO));
    assert!(d.closed());
    assert!(matches!(d.take_action(), Some(ViewerAction::WithHost(_))));
}

/// The footer's buttons: Push (in the accent, no `↵` on it), Cancel.
#[test]
fn its_buttons_are_in_the_footer_push_without_a_key() {
    let mut d = dialog();
    let buttons = d.modal_buttons();
    assert_eq!(buttons.iter().map(|b| b.id).collect::<Vec<_>>(), [GO, CANCEL]);
    assert!(buttons[0].main && buttons[0].keyless && buttons[0].enabled);
    d.form.as_mut().unwrap().lines[0].push = false;
    assert!(!d.modal_buttons()[0].enabled, "nothing chosen: not pressed");
    assert!(d.command(CANCEL));
    assert!(matches!(d.take_action(), Some(ViewerAction::Close)));
}

#[test]
fn a_force_push_is_confirmed_first() {
    let mut d = dialog();
    d.form.as_mut().unwrap().force = true;
    d.command(GO);
    assert!(d.confirming && !d.closed() && d.take_action().is_none());
    assert!(d.modal_buttons()[0].danger, "asked again in the danger color");
    d.command(GO);
    assert!(d.closed());
}

#[test]
fn a_change_after_asking_asks_again() {
    let mut d = dialog();
    d.form.as_mut().unwrap().force = true;
    d.command(GO);
    d.apply(Hit::Tags, Point::new(0.0, 0.0), 1);
    assert!(!d.confirming);
}

#[test]
fn nothing_goes_with_nothing_chosen() {
    let mut d = dialog();
    d.form.as_mut().unwrap().lines[0].push = false;
    d.command(GO);
    assert!(!d.closed());
}

/// Opened with the current branch far down the list: the rows start so it's in sight.
#[test]
fn the_branch_pushed_is_in_sight_when_it_opens() {
    let mut d = PushDialog::default();
    let branches: Vec<String> = (0..20).map(|i| format!("feature/{i:02}")).collect();
    d.set_data(&PushRequest {
        repo: Arc::new(FakeRepository::default()),
        remotes: vec![Remote { name: "origin".into(), url: "git@host:a.git".into() }],
        branches,
        states: Vec::new(),
        branch: Some("feature/15".into()),
        remote: None,
    });
    let at = d.form.as_ref().unwrap().lines.iter().position(|l| l.push).unwrap();
    assert!((d.first..d.first + ROWS_SHOWN).contains(&at), "{at} in sight from {}", d.first);
}

#[test]
fn tab_keeps_the_row_typed_in_shown() {
    let mut d = dialog();
    d.edit_key(ViewerKey::Tab);
    assert_eq!(d.typing, Some(0));
    assert_eq!(d.first, 0);
}

/// Many branches: the app's scrollbar in the margin right of the table; its thumb dragged
/// moves the rows a row at a time (never past the end), ticking nothing.
#[test]
fn the_table_s_thumb_is_dragged_a_row_at_a_time() {
    let mut d = PushDialog::default();
    let branches: Vec<String> = (0..20).map(|i| format!("feature/{i:02}")).collect();
    d.set_data(&PushRequest {
        repo: Arc::new(FakeRepository::default()),
        remotes: vec![Remote { name: "origin".into(), url: "git@host:a.git".into() }],
        branches,
        states: Vec::new(),
        branch: Some("feature/00".into()),
        remote: None,
    });
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 720.0, 520.0);
    let mut scene = render_api::Scene::default();
    d.paint(ViewerPaint {
        bounds,
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert_eq!(d.first, 0);
    let ticked = |d: &PushDialog| d.form.as_ref().unwrap().lines.iter().filter(|l| l.push).count();
    let before = ticked(&d);
    let (lane, _) = d.bar_area.expect("a scrollbar: more branches than shown");
    let y = lane.min_y() + 6.0;
    let x = (0..16)
        .map(|dx| lane.max_x() - 1.0 - dx as f32)
        .find(|x| {
            Viewer::press(&mut d, Point::new(*x, y), 1);
            d.bar.is_dragging()
        })
        .expect("the thumb at the table's top");
    assert!(d.drag(Point::new(x, y + 90.0)) && d.first > 0);
    d.drag(Point::new(x, y + 10_000.0));
    assert_eq!(d.first, 20 - ROWS_SHOWN, "at its end, no further");
    assert!(d.release());
    assert_eq!(ticked(&d), before, "nothing ticked by it");
}
