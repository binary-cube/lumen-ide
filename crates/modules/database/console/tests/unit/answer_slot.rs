use super::*;

#[test]
fn it_is_taken_once_whichever_clone_asks() {
    let once = Once::new(7);
    let other = once.clone();
    assert_eq!(other.take(), Some(7));
    assert_eq!(once.take(), None);
}

#[test]
fn it_equals_only_its_own_clones() {
    let once = Once::new(1);
    assert_eq!(once, once.clone());
    assert_ne!(once, Once::new(1));
}
