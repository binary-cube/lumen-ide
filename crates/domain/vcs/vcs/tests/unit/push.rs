use super::*;

#[test]
fn a_password_in_an_address_is_never_shown() {
    assert_eq!(url_without_password("https://me:secret@host.io/a.git"), "https://me@host.io/a.git");
    assert_eq!(url_without_password("https://token@host.io/a.git"), "https://token@host.io/a.git");
}

#[test]
fn other_addresses_stay_as_they_are() {
    for url in
        ["git@github.com:me/a.git", "https://host.io/a@b.git", "ssh://git@host/a.git", "/srv/a.git"]
    {
        assert_eq!(url_without_password(url), url);
    }
}

#[test]
fn a_plan_has_work_with_a_branch_or_the_tags() {
    let mut plan =
        PushPlan { remote: "origin".into(), branches: Vec::new(), tags: false, force: false };
    assert!(!plan.has_work());
    plan.tags = true;
    assert!(plan.has_work());
}
