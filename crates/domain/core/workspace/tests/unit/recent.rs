use super::*;

fn list(paths: &[&str]) -> RecentProjects {
    RecentProjects::of(paths.iter().map(PathBuf::from).collect())
}

fn names(list: &RecentProjects, group: GroupId) -> Vec<String> {
    list.projects_in(group).iter().map(|p| p.display().to_string()).collect()
}

#[test]
fn most_recent_first_without_duplicates() {
    let mut recent = RecentProjects::default();
    recent.open(Path::new("/w/a"));
    recent.open(Path::new("/w/b"));
    recent.open(Path::new("/w/a"));
    assert_eq!(names(&recent, RECENT), ["/w/a", "/w/b"]);
    for i in 0..30 {
        recent.open(Path::new(&format!("/p{i}")));
    }
    assert_eq!(recent.projects.len(), LIMIT);
    assert_eq!(recent.projects[0], PathBuf::from("/p29"));
}

#[test]
fn the_recent_group_is_always_there_and_stays() {
    let mut recent = RecentProjects::default();
    assert_eq!(recent.groups.iter().map(|g| g.id).collect::<Vec<_>>(), [RECENT]);
    assert!(!recent.delete_group(RECENT));
    assert!(!recent.rename_group(RECENT, "Mine"));
    let g = recent.new_group("G", Some(RECENT));
    assert_eq!(recent.group(g).and_then(|g| g.parent), None, "no subgroups in it");
    assert!(!recent.move_group(g, Some(RECENT), None));
    // It moves: after G, then into G.
    assert!(recent.move_group(RECENT, None, None));
    assert_eq!(recent.children(None).map(|g| g.id).collect::<Vec<_>>(), [g, RECENT]);
    assert!(recent.move_group(RECENT, Some(g), None));
    assert!(recent.is_within(RECENT, g));
}

#[test]
fn the_limit_counts_only_the_recent_group() {
    let mut recent = list(&["/kept"]);
    let group = recent.new_group("Clients", None);
    assert!(recent.move_to(Path::new("/kept"), group, Place::Anywhere));
    for i in 0..30 {
        recent.open(Path::new(&format!("/p{i}")));
    }
    assert_eq!(recent.projects.len(), LIMIT + 1);
    recent.open(Path::new("/kept"));
    assert_eq!(recent.group_of(Path::new("/kept")), group);
}

#[test]
fn groups_list_the_most_recently_opened_first() {
    let mut recent = list(&["/a", "/b", "/c"]);
    let g = recent.new_group("G", None);
    for p in ["/a", "/b", "/c"] {
        recent.move_to(Path::new(p), g, Place::Anywhere);
    }
    assert_eq!(names(&recent, g), ["/a", "/b", "/c"]);
    recent.open(Path::new("/c"));
    assert_eq!(names(&recent, g), ["/c", "/a", "/b"]);
}

#[test]
fn a_chosen_place_keeps_the_order_chosen() {
    let mut recent = list(&["/a", "/b", "/c", "/d"]);
    let g = recent.new_group("G", None);
    for p in ["/a", "/b", "/c"] {
        recent.move_to(Path::new(p), g, Place::Anywhere);
    }
    // Placed before /a: the group keeps this order from now on.
    assert!(recent.move_to(Path::new("/c"), g, Place::Before(PathBuf::from("/a"))));
    assert_eq!(names(&recent, g), ["/c", "/a", "/b"]);
    recent.open(Path::new("/b"));
    assert_eq!(names(&recent, g), ["/c", "/a", "/b"], "opening doesn't move it");
    // Dropped on the group: at the end; at the end, chosen: the same.
    recent.move_to(Path::new("/d"), g, Place::Anywhere);
    assert_eq!(names(&recent, g), ["/c", "/a", "/b", "/d"]);
    recent.move_to(Path::new("/c"), g, Place::End);
    assert_eq!(names(&recent, g), ["/a", "/b", "/d", "/c"]);
    // Sorted by opening again.
    assert!(recent.sort_by_opening(g));
    assert_eq!(names(&recent, g), ["/b", "/a", "/c", "/d"]);
    // The recent group never keeps an order.
    recent.move_to(Path::new("/a"), RECENT, Place::End);
    assert_eq!(recent.group(RECENT).map(|g| g.order), Some(Order::Recent));
}

#[test]
fn projects_move_between_groups() {
    let mut recent = list(&["/a", "/b"]);
    let clients = recent.new_group("Clients", None);
    let inner = recent.new_group("Northwind", Some(clients));
    assert!(recent.move_to(Path::new("/a"), inner, Place::Anywhere));
    assert!(recent.move_to(Path::new("/b"), clients, Place::Anywhere));
    assert_eq!(recent.count_in(clients), 2);
    assert_eq!(recent.count_in(inner), 1);
    assert_eq!(recent.path_of(inner), ["Clients", "Northwind"]);
    assert!(recent.projects_in(RECENT).is_empty());
    assert!(recent.move_to(Path::new("/a"), RECENT, Place::Anywhere));
    assert_eq!(names(&recent, RECENT), ["/a"]);
    assert!(!recent.move_to(Path::new("/c"), clients, Place::Anywhere));
    assert!(!recent.move_to(Path::new("/a"), GroupId::new(), Place::Anywhere));
}

#[test]
fn a_group_never_goes_into_itself_and_takes_its_place() {
    let mut recent = RecentProjects::default();
    let a = recent.new_group("A", None);
    let b = recent.new_group("B", Some(a));
    let c = recent.new_group("C", Some(b));
    assert!(!recent.move_group(a, Some(a), None));
    assert!(!recent.move_group(a, Some(c), None));
    assert!(recent.move_group(c, None, Some(RECENT)));
    assert_eq!(recent.children(None).map(|g| g.id).collect::<Vec<_>>(), [c, RECENT, a]);
    assert!(recent.move_group(a, Some(c), None));
    assert_eq!(recent.group(a).and_then(|g| g.parent), Some(c));
}

#[test]
fn deleting_a_group_keeps_what_it_held() {
    let mut recent = list(&["/a", "/b"]);
    let top = recent.new_group("Top", None);
    let mid = recent.new_group("Mid", Some(top));
    let low = recent.new_group("Low", Some(mid));
    recent.move_to(Path::new("/a"), mid, Place::Anywhere);
    recent.move_to(Path::new("/b"), top, Place::Anywhere);
    assert!(recent.delete_group(mid));
    assert_eq!(recent.group_of(Path::new("/a")), top);
    assert_eq!(recent.group(low).and_then(|g| g.parent), Some(top));
    assert!(recent.delete_group(top));
    assert_eq!(recent.group_of(Path::new("/a")), RECENT);
    assert_eq!(recent.group(low).and_then(|g| g.parent), None);
    assert_eq!(recent.projects.len(), 2);
}

#[test]
fn forgetting_takes_a_project_out() {
    let mut recent = list(&["/a", "/b"]);
    let g = recent.new_group("G", None);
    recent.move_to(Path::new("/a"), g, Place::End);
    assert!(recent.forget(Path::new("/a")));
    assert!(!recent.forget(Path::new("/a")));
    assert_eq!(recent.projects, [PathBuf::from("/b")]);
    assert!(recent.placed.is_empty());
    assert!(recent.group(g).is_some_and(|g| g.manual.is_empty()));
}

#[test]
fn groups_are_renamed_and_collapsed() {
    let mut recent = RecentProjects::default();
    let g = recent.new_group("  Old ", None);
    assert_eq!(recent.group(g).map(|g| g.name.as_str()), Some("Old"));
    assert!(!recent.rename_group(g, "   "));
    assert!(recent.rename_group(g, "New"));
    assert!(recent.set_collapsed(g, true));
    assert!(!recent.set_collapsed(g, true));
    assert_eq!(recent.group(g).map(|g| (g.name.as_str(), g.collapsed)), Some(("New", true)));
}

#[test]
fn a_broken_list_is_repaired() {
    let mut recent = list(&["/a"]);
    let a = recent.new_group("A", None);
    let b = recent.new_group("B", Some(a));
    let orphan = recent.new_group("Orphan", None);
    // A cycle, a missing parent, a project in a missing group, one not in the list, the
    // recent group gone.
    recent.groups.iter_mut().filter(|g| g.id == a).for_each(|g| g.parent = Some(b));
    recent
        .groups
        .iter_mut()
        .filter(|g| g.id == orphan)
        .for_each(|g| g.parent = Some(GroupId::new()));
    recent.placed.insert(PathBuf::from("/a"), GroupId::new());
    recent.placed.insert(PathBuf::from("/gone"), a);
    recent.groups.retain(|g| g.id != RECENT);
    let recent = recent.repaired();
    assert!(!recent.is_within(a, b) || !recent.is_within(b, a));
    assert_eq!(recent.group(orphan).and_then(|g| g.parent), None);
    assert!(recent.placed.is_empty());
    assert_eq!(recent.groups.first().map(|g| g.id), Some(RECENT));
}

#[test]
fn clearing_the_recent_ones_keeps_the_groups_projects() {
    let mut recent = list(&["/a", "/kept", "/b"]);
    let group = recent.new_group("Clients", None);
    assert!(recent.move_to(Path::new("/kept"), group, Place::Anywhere));
    assert!(recent.clear_recent());
    assert_eq!(names(&recent, RECENT), Vec::<String>::new());
    assert_eq!(names(&recent, group), ["/kept"]);
    assert!(!recent.clear_recent(), "nothing left to clear");
}
