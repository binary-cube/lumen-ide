use super::*;

#[test]
fn parses_compound_values() {
    assert_eq!(parse_splits("right, move-down"), vec![(Split::Right, false), (Split::Down, true)]);
    let drag = parse_tab_drag("10,20,30,40!");
    assert!(drag.is_some_and(|d| d.drop && d.to == Point::new(30.0, 40.0)));
    assert!(parse_tab_drag("1,2,3").is_none());
    let compare = parse_compare("a.php,b.php,merge");
    assert_eq!(compare, Some((PathBuf::from("a.php"), PathBuf::from("b.php"), true)));
}
