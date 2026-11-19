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
