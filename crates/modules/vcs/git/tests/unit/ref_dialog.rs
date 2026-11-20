use super::*;
use crate::fake::FakeRepository;

fn dialog(kind: RefKindAsked, taken: &[&str]) -> RefDialog {
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository::default());
    let mut d = RefDialog::default();
    d.set_data(&RefRequest { repo, kind, taken: taken.iter().map(|s| (*s).to_owned()).collect() });
    d
}

/// The name is checked as git checks references' names, and against those taken; the same
/// name again isn't a rename.
#[test]
fn the_name_is_checked() {
    let mut d =
        dialog(RefKindAsked::NewBranch { from: None, from_label: "HEAD".into() }, &["main"]);
    assert_eq!(d.problem(), Some(t("git.ref_empty")));
    d.text_input("feature/login");
    assert_eq!(d.problem(), None);
    d.command("select_all");
    d.text_input("main");
    assert_eq!(d.problem(), Some(t("git.ref_branch_exists")));
    d.command("select_all");
    d.text_input("my branch");
    assert_eq!(d.problem(), Some(t("git.ref_bad_character")));
    d.command("select_all");
    d.text_input("a..b");
    assert_eq!(d.problem(), Some(t("git.ref_bad_shape")));
    assert!(!d.give(), "not with a bad name");
    let mut r = dialog(RefKindAsked::RenameBranch { old: "topic".into() }, &["topic", "main"]);
    assert_eq!(r.name.text(), "topic", "its name to edit");
    assert_eq!(r.problem(), Some(t("git.ref_same")));
    r.command("select_all");
    r.text_input("main");
    assert_eq!(r.problem(), Some(t("git.ref_branch_exists")));
    let mut tag = dialog(RefKindAsked::NewTag { on: None, on_label: "HEAD".into() }, &["v1"]);
    tag.text_input("v1");
    assert_eq!(tag.problem(), Some(t("git.ref_tag_exists")));
}

/// A dialog starts where it was asked from; without it, `HEAD`.
#[test]
fn a_dialog_starts_from_what_it_was_asked_with() {
    let repo = FakeRepository::default();
    let commit = Target::Commit { id: "abc".into(), short: "ab".into() };
    let Some((RefKindAsked::NewTag { on, on_label }, true)) = new_tag_on(&repo, Some(commit))
    else {
        panic!("a tag on the commit")
    };
    assert_eq!((on.as_deref(), on_label.as_str()), (Some("abc"), "ab"));
    let local = Target::Reference { name: "topic".into(), local_branch: true };
    let Some((RefKindAsked::RenameBranch { old }, false)) = rename_on(&repo, Some(local)) else {
        panic!("that branch renamed")
    };
    assert_eq!(old, "topic");
}

/// A tag's two fields: a press in the message's gives it the keys and a drag there selects
/// in it, the name's left as it was.
#[test]
fn a_press_and_a_drag_in_a_field_select_in_it() {
    let mut d = dialog(RefKindAsked::NewTag { on: None, on_label: "HEAD".into() }, &[]);
    d.text_input("v2");
    assert!(d.edit_key(ViewerKey::Tab), "Tab: to the message's field");
    d.text_input("the release");
    assert!(d.edit_key(ViewerKey::Tab), "and back to the name's");
    assert_eq!(d.controls.focused(), Some(NAME));
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    d.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 520.0, 300.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let (field, _) = *d.hits.iter().find(|(_, h)| *h == Hit::Message).unwrap();
    let mid = field.min_y() + field.size.height / 2.0;
    d.press(Point::new(field.max_x() - 4.0, mid), 1);
    assert_eq!(d.controls.focused(), Some(MESSAGE));
    assert!(d.drag(Point::new(field.min_x() + 4.0, mid)));
    d.release();
    assert_eq!(d.selected_text().as_deref(), Some("the release"));
    assert!(d.name.selected_text().is_empty());
}

/// A branch's dialog: the name's field has the keys from the start and keeps them when the
/// checkbox is pressed; Tab is no move (one field).
#[test]
fn the_name_keeps_the_keys_through_a_press_on_the_checkbox() {
    let mut d =
        dialog(RefKindAsked::NewBranch { from: None, from_label: "HEAD".into() }, &["main"]);
    assert_eq!(d.controls.focused(), Some(NAME));
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    d.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 520.0, 300.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let (row, _) = *d.hits.iter().find(|(_, h)| *h == Hit::Checkout).unwrap();
    d.press(row.center(), 1);
    assert!(!d.checkout, "the box unticked");
    assert_eq!(d.controls.focused(), Some(NAME), "the name kept the keys");
    assert!(!d.edit_key(ViewerKey::Tab));
    assert!(d.text_input("topic"));
    assert!(d.edit_key(ViewerKey::Backspace));
    assert_eq!(d.name.text(), "topi");
}
