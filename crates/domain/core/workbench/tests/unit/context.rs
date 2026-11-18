use super::*;
use workspace::Mark;

#[test]
fn an_entry_s_mark_becomes_its_fact_and_providers_add_theirs() {
    let absent = Target::new("/p/a.php".into(), false).with_mark(Mark::None, true);
    let changed = Target::new("/p/b.php".into(), false).with_mark(Mark::Changed, false);
    let fine = Target::new("/p/c".into(), true).with_mark(Mark::None, false);
    assert!(absent.has(facts::ABSENT) && changed.has(facts::CHANGED) && fine.facts.is_empty());
    let mut ctx = ActionContext::new(MenuPlace::TreeFile, vec![absent, changed, fine]);
    assert_eq!(ctx.path(), Some(Path::new("/p/a.php")));
    assert!(ctx.any(facts::ABSENT) && !ctx.all(facts::ABSENT));
    // A provider's fact, once per target; an unknown path is ignored.
    ctx.add(Path::new("/p/c"), "remote.excluded");
    ctx.add(Path::new("/p/c"), "remote.excluded");
    ctx.add(Path::new("/elsewhere"), "remote.excluded");
    assert_eq!(ctx.targets[2].facts, ["remote.excluded"]);
    assert!(!ActionContext::empty(MenuPlace::Editor).all(facts::ABSENT));
}
