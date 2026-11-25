use super::*;
use workbench_model::Notification;

fn window(focus_turn: u64) -> Candidate {
    Candidate { focus_turn, shows: false }
}

/// A new one goes to the window the user is in: focused (the highest turn), else the one
/// focused last — never another, and to one window only.
#[test]
fn a_new_one_goes_to_the_window_the_user_is_in() {
    let told = Told::Show(Notification::info("an update"));
    assert_eq!(target(&told, &[window(3), window(7), window(5)]), Some(1));
    // The focused one closed: the one focused before it.
    assert_eq!(target(&told, &[window(3), window(5)]), Some(1));
    assert_eq!(target(&told, &[window(1)]), Some(0), "the only one");
    assert_eq!(target(&told, &[]), None, "no window: it waits");
}

/// One shown already (its progress, its end, its closing) stays on the window showing it,
/// wherever the focus went; gone from every window, an update goes to the one the user is
/// in and a closing to none.
#[test]
fn one_shown_stays_where_it_shows() {
    let progress = Notification::progress("downloading");
    let shown = [window(9), Candidate { focus_turn: 2, shows: true }];
    assert_eq!(target(&Told::Update(progress.clone()), &shown), Some(1));
    assert_eq!(target(&Told::Dismiss(progress.id), &shown), Some(1));
    assert_eq!(target(&Told::Show(progress.clone()), &shown), Some(1), "merged where it is");
    let nowhere = [window(9), window(2)];
    assert_eq!(target(&Told::Update(progress.clone()), &nowhere), Some(0));
    assert_eq!(target(&Told::Dismiss(progress.id), &nowhere), None);
}

/// One the user asked for goes to the window they're in, even when another shows it.
#[test]
fn one_asked_for_goes_where_the_user_is() {
    let checking = Notification::progress("looking for a newer version");
    let shown_elsewhere = [Candidate { focus_turn: 1, shows: true }, window(4)];
    assert_eq!(target(&Told::Bring(checking.clone()), &shown_elsewhere), Some(1));
    assert_eq!(target(&Told::Bring(checking), &[window(2), window(1)]), Some(0));
}
