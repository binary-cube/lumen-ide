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
