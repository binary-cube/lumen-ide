use super::*;

fn folder_in(name: &str, parent: Option<&SourceFolder>) -> SourceFolder {
    let mut f = SourceFolder::new(name, Scope::Project);
    f.parent = parent.map(|p| p.id);
    f
}

#[test]
fn a_color_reads_back_from_its_name() {
    for color in SourceColor::NAMED {
        assert_eq!(SourceColor::parse(&color.name()), Some(color));
    }
    assert_eq!(SourceColor::parse("RED"), Some(SourceColor::Red));
    assert_eq!(SourceColor::parse("grey"), Some(SourceColor::Gray));
    let picked = SourceColor::Custom(0x123456);
    assert_eq!(SourceColor::parse(&picked.name()), Some(picked));
    assert_eq!(SourceColor::parse("crimson"), None);
    assert_eq!(SourceColor::parse("#zz"), None);
}

#[test]
fn a_source_takes_the_nearest_folders_color_unless_it_has_its_own() {
    let mut prod = folder_in("prod", None);
    prod.color = Some(SourceColor::Red);
    let eu = folder_in("eu", Some(&prod));
    let mut staging = folder_in("staging", Some(&eu));
    staging.color = Some(SourceColor::Orange);
    let folders = vec![prod, eu.clone(), staging.clone()];

    let mut source = DataSource::new("shop", "postgres", Scope::Project);
    assert_eq!(effective_color(&source, &folders), None, "no folder, no color");
    source.folder = Some(eu.id);
    assert_eq!(effective_color(&source, &folders), Some(SourceColor::Red), "from up the chain");
    source.folder = Some(staging.id);
    assert_eq!(effective_color(&source, &folders), Some(SourceColor::Orange), "the nearest");
    source.color = Some(SourceColor::Green);
    assert_eq!(effective_color(&source, &folders), Some(SourceColor::Green), "its own first");
    // A folder gone: no color from it.
    source.color = None;
    source.folder = Some(FolderId::new());
    assert_eq!(effective_color(&source, &folders), None);

    let path: Vec<&str> = folder_path(&folders, staging.id).iter().map(|f| &*f.name).collect();
    assert_eq!(path, ["prod", "eu", "staging"]);
}

#[test]
fn a_folder_never_goes_into_itself_nor_one_inside_it() {
    let a = folder_in("a", None);
    let b = folder_in("b", Some(&a));
    let c = folder_in("c", Some(&b));
    let other = SourceFolder::new("global", Scope::Global);
    let mut folders = vec![a.clone(), b.clone(), c.clone(), other.clone()];
    assert_eq!(descendants(&folders, a.id), vec![b.id, c.id]);
    assert!(!can_move_folder(&folders, a.id, Some(a.id)));
    assert!(!can_move_folder(&folders, a.id, Some(c.id)), "a loop");
    assert!(!can_move_folder(&folders, c.id, Some(other.id)), "another list's");
    assert!(!move_folder(&mut folders, a.id, Some(b.id)));
    assert!(move_folder(&mut folders, c.id, None), "to the top");
    assert!(move_folder(&mut folders, a.id, Some(c.id)), "under what was inside it");
    assert_eq!(folder(&folders, a.id).unwrap().parent, Some(c.id));
    assert!(!move_folder(&mut folders, a.id, Some(c.id)), "already there");
}

#[test]
fn deleting_a_folder_moves_what_it_held_up_and_keeps_every_source() {
    let a = folder_in("a", None);
    let b = folder_in("b", Some(&a));
    let c = folder_in("c", Some(&b));
    let mut folders = vec![a.clone(), b.clone(), c.clone()];
    let mut in_b = DataSource::new("in b", "sqlite", Scope::Project);
    in_b.folder = Some(b.id);
    let mut in_c = DataSource::new("in c", "sqlite", Scope::Project);
    in_c.folder = Some(c.id);
    let mut sources = vec![in_b, in_c];
    assert!(delete_folder(&mut folders, &mut sources, b.id));
    assert_eq!(sources.len(), 2, "no source goes with it");
    assert_eq!(sources[0].folder, Some(a.id), "its sources into its parent");
    assert_eq!(sources[1].folder, Some(c.id));
    assert_eq!(folder(&folders, c.id).unwrap().parent, Some(a.id), "its subfolders too");
    assert!(delete_folder(&mut folders, &mut sources, a.id));
    assert_eq!(sources[0].folder, None, "to the top");
    assert_eq!(folder(&folders, c.id).unwrap().parent, None);
    assert!(!delete_folder(&mut folders, &mut sources, a.id), "gone already");
}

#[test]
fn a_list_read_back_is_made_sound() {
    let mut a = folder_in("a", None);
    let mut b = folder_in("b", Some(&a));
    a.parent = Some(b.id); // a loop
    let lost = folder_in("lost", Some(&SourceFolder::new("gone", Scope::Project)));
    b.name = "b".into();
    let global = SourceFolder::new("global", Scope::Global);
    let mut wrong_list = DataSource::new("x", "sqlite", Scope::Project);
    wrong_list.folder = Some(global.id);
    let mut fine = DataSource::new("y", "sqlite", Scope::Project);
    fine.folder = Some(b.id);
    let mut list =
        SourceList::new(vec![wrong_list, fine], vec![a.clone(), b.clone(), lost.clone(), global]);
    assert!(repair(&mut list));
    let parent = |id| folder(&list.folders, id).unwrap().parent;
    assert!(parent(a.id).is_none() || parent(b.id).is_none(), "the loop broken");
    assert_eq!(parent(lost.id), None, "its parent isn't there");
    assert_eq!(list.sources[0].folder, None, "another list's folder");
    assert_eq!(list.sources[1].folder, Some(b.id));
    assert!(!repair(&mut list), "sound now");
}

#[test]
fn a_folder_counts_the_sources_inside_it_and_lists_its_children_by_name() {
    let top = folder_in("top", None);
    let zoe = folder_in("Zoe", Some(&top));
    let alpha = folder_in("alpha", Some(&top));
    let folders = vec![top.clone(), zoe.clone(), alpha.clone()];
    let mut one = DataSource::new("one", "sqlite", Scope::Project);
    one.folder = Some(zoe.id);
    let mut two = DataSource::new("two", "sqlite", Scope::Project);
    two.folder = Some(top.id);
    let three = DataSource::new("three", "sqlite", Scope::Project);
    let sources = vec![one, two, three];
    assert_eq!(sources_in(&folders, &sources, top.id).len(), 2);
    assert_eq!(sources_in(&folders, &sources, alpha.id).len(), 0);
    let names: Vec<&str> =
        children(&folders, Scope::Project, Some(top.id)).iter().map(|f| &*f.name).collect();
    assert_eq!(names, ["alpha", "Zoe"]);
    assert!(children(&folders, Scope::Global, None).is_empty());
}

#[test]
fn only_where_a_source_is_shown_keeps_its_connection() {
    let a = DataSource::new("shop", "postgres", Scope::Project);
    let mut b = a.clone();
    b.color = Some(SourceColor::Red);
    b.folder = Some(FolderId::new());
    assert!(a.same_connection(&b));
    b.target.host = "db".into();
    assert!(!a.same_connection(&b));
}

#[test]
fn production_is_inherited_from_any_folder_up_and_never_taken_away() {
    let mut prod = folder_in("prod", None);
    let eu = folder_in("eu", Some(&prod));
    let dev = folder_in("dev", None);
    let mut source = DataSource::new("shop", "postgres", Scope::Project);
    let folders = vec![prod.clone(), eu.clone(), dev.clone()];
    assert!(!effective_production(&source, &folders), "nothing marked");
    source.folder = Some(eu.id);
    assert!(!effective_production(&source, &folders));
    prod.production = true;
    let folders = vec![prod.clone(), eu.clone(), dev.clone()];
    assert!(effective_production(&source, &folders), "from up the chain");
    assert_eq!(production_folder(&folders, eu.id).map(|f| f.id), Some(prod.id));
    assert!(folder_production(&folders, eu.id) && !folder_production(&folders, dev.id));
    // Its own mark, anywhere.
    source.folder = Some(dev.id);
    assert!(!effective_production(&source, &folders));
    source.production = true;
    assert!(effective_production(&source, &folders));
    // A folder gone: nothing from it.
    source.production = false;
    source.folder = Some(eu.id);
    assert!(!effective_production(&source, std::slice::from_ref(&eu)));
}

#[test]
fn the_production_mark_keeps_the_connection() {
    let a = DataSource::new("shop", "postgres", Scope::Project);
    let mut b = a.clone();
    b.production = true;
    assert!(a.same_connection(&b), "only how carefully it's used changed");
}
