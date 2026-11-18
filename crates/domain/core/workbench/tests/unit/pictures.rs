use super::*;

// One test: the pictures are process-wide (windows far from the others' numbers).
#[test]
fn a_window_keeps_its_latest_picture_until_its_project_closes() {
    let (a, b) = (9_001, 9_002);
    assert!(of(a).is_none());
    keep(a, 2, 1, vec![1; 8]);
    let first = of(a).expect("kept");
    assert_eq!((first.width, first.height), (2, 1));
    keep(a, 1, 1, vec![2; 4]);
    keep(b, 1, 1, vec![3; 4]);
    let second = of(a).expect("kept");
    assert!(second.taken > first.taken, "a later one says so");
    assert_eq!(&*second.rgba, &[2; 4]);
    keep_only(&[b]);
    assert!(of(a).is_none(), "its project closed: its picture went");
    assert!(of(b).is_some());
    keep_only(&[]);
}
