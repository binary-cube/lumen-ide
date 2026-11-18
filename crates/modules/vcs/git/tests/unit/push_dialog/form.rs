use std::sync::Arc;

use super::*;
use crate::fake::FakeRepository;

fn remote(name: &str) -> Remote {
    Remote { name: name.into(), url: format!("git@host:{name}.git") }
}

fn tracking(branch: &str, upstream: &str) -> SyncState {
    SyncState { branch: branch.into(), upstream: Some(upstream.into()), ahead: 0, behind: 0 }
}

fn request() -> PushRequest {
    PushRequest {
        repo: Arc::new(FakeRepository::default()),
        remotes: vec![remote("fork"), remote("origin")],
        branches: vec!["main".into(), "topic".into(), "other".into()],
        states: vec![tracking("main", "origin/trunk"), tracking("other", "fork/other")],
        branch: Some("main".into()),
        remote: None,
    }
}

#[test]
fn it_starts_with_the_branch_on_what_it_tracks() {
    let form = PushForm::new(&request());
    assert_eq!(form.remote_name(), "origin");
    let main = &form.lines[0];
    assert!(main.push && main.track);
    assert_eq!(main.remote_name.text(), "trunk");
    assert!(!form.lines[1].push, "only the current branch");
    assert!(form.lines[1].track, "a branch tracking nothing starts tracking");
    assert!(!form.lines[2].track, "tracking another remote, it keeps doing so");
}

#[test]
fn a_remote_asked_for_is_the_one_chosen() {
    let form = PushForm::new(&PushRequest { remote: Some("fork".into()), ..request() });
    assert_eq!(form.remote_name(), "fork");
    assert_eq!(form.lines[0].remote_name.text(), "main");
    assert!(!form.lines[0].track);
}

#[test]
fn another_remote_changes_only_the_names_nobody_typed() {
    let mut form = PushForm::new(&request());
    form.lines[1].remote_name = TextInput::new("mine");
    form.choose_remote(0);
    assert_eq!(form.lines[0].remote_name.text(), "main");
    assert_eq!(form.lines[1].remote_name.text(), "mine");
    assert!(form.lines[2].track);
}

#[test]
fn select_all_takes_all_or_none() {
    let mut form = PushForm::new(&request());
    assert_eq!(form.all_state(), Check::Mixed);
    form.toggle_all();
    assert_eq!(form.all_state(), Check::On);
    form.toggle_all();
    assert_eq!(form.all_state(), Check::Off);
}

#[test]
fn it_goes_only_with_something_to_send_under_good_names() {
    let mut form = PushForm::new(&request());
    assert_eq!(form.problem(), None);
    form.lines[0].remote_name = TextInput::new("a..b");
    assert_eq!(form.problem(), Some(t("git.ref_bad_shape")));
    form.lines[0].push = false;
    assert_eq!(form.problem(), Some(t("git.push_nothing_chosen")));
    form.tags = true;
    assert_eq!(form.problem(), None);
}

#[test]
fn the_plan_is_what_was_chosen() {
    let mut form = PushForm::new(&request());
    form.force = true;
    let plan = form.plan();
    assert_eq!(plan.remote, "origin");
    assert_eq!(
        plan.branches,
        [PushedBranch { local: "main".into(), remote: "trunk".into(), track: true }]
    );
    assert!(plan.force && !plan.tags);
}
