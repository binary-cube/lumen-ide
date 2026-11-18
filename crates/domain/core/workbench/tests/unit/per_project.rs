use super::*;

#[test]
fn a_project_s_state_is_found_while_it_lives() {
    let table = PerProject::new();
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let (one, two) = (Arc::new(1), Arc::new(2));
    table.insert(a, &one);
    table.insert(b, &two);
    assert_eq!(table.get(a).as_deref(), Some(&1));
    assert_eq!(table.all().len(), 2);
    drop(one);
    assert_eq!(table.get(a), None, "gone with its holder");
    table.remove(b);
    assert!(table.all().is_empty());
}
