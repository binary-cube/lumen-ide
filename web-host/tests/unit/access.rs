use serde_json::json;

use super::*;

/// The probe's page as Chromium told it: a heading, a text, a button, a link under ignored
/// containers, each with its text box.
fn page() -> Json {
    json!({"updates": [{
        "root_id": 1,
        "tree_data": {"focus_id": 7},
        "nodes": [
            {"id": 1, "role": "rootWebArea", "child_ids": [2], "offset_container_id": 0,
             "location": {"x": 0.0, "y": 0.0, "width": 400.0, "height": 300.0},
             "attributes": {"scrollY": 10}},
            {"id": 2, "role": "genericContainer", "state": ["ignored"], "child_ids": [5, 6, 7],
             "offset_container_id": 1, "location": {"x": 0.0, "y": 0.0, "width": 400.0, "height": 300.0}},
            {"id": 5, "role": "heading", "child_ids": [9], "offset_container_id": 1,
             "location": {"x": 16.0, "y": 16.0, "width": 368.0, "height": 74.0},
             "attributes": {"name": "Title", "hierarchicalLevel": 1}},
            {"id": 9, "role": "staticText", "child_ids": [-2], "offset_container_id": 1,
             "attributes": {"name": "Title"}},
            {"id": -2, "role": "inlineTextBox", "offset_container_id": 9},
            {"id": 6, "role": "paragraph", "child_ids": [10], "offset_container_id": 1},
            {"id": 10, "role": "staticText", "offset_container_id": 1,
             "attributes": {"name": "Some text"}},
            {"id": 7, "role": "checkBox", "state": ["focusable"], "offset_container_id": 1,
             "location": {"x": 16.0, "y": 200.0, "width": 20.0, "height": 20.0},
             "attributes": {"name": "Agree", "checkedState": "true", "restriction": "disabled"}}
        ]
    }]})
}

/// What a screen reader reads: no scaffolding, no label said twice, parents first, places
/// moved by the page's scroll, the focus and the states.
#[test]
fn only_what_a_screen_reader_reads_is_told() {
    let mut tree = Tree::default();
    tree.apply(&page());
    let told = tree.told();
    let shown: Vec<_> =
        told.iter().map(|n| (n.id, n.parent, n.role.as_str(), n.name.as_str())).collect();
    assert_eq!(
        shown,
        [
            (5, None, "heading", "Title"),
            (6, None, "paragraph", ""),
            (10, Some(6), "staticText", "Some text"),
            (7, None, "checkBox", "Agree"),
        ]
    );
    let heading = &told[0];
    assert_eq!(heading.rect, [16.0, 6.0, 368.0, 74.0], "moved by the page's scroll");
    assert_eq!(heading.level, Some(1));
    let check = &told[3];
    assert!(check.focused && check.disabled);
    assert_eq!(check.checked, Some(true));
}

/// An update replaces its nodes; what's cleared or unreachable goes.
#[test]
fn updates_replace_and_clear() {
    let mut tree = Tree::default();
    tree.apply(&page());
    tree.apply(&json!({"updates": [{
        "node_id_to_clear": 2,
        "nodes": [{"id": 2, "role": "genericContainer", "state": ["ignored"], "child_ids": [7],
                   "offset_container_id": 1},
                  {"id": 7, "role": "button", "offset_container_id": 1,
                   "attributes": {"name": "Go"}}]
    }]}));
    let told: Vec<_> = tree.told().into_iter().map(|n| (n.id, n.name)).collect();
    assert_eq!(told, [(7, "Go".to_owned())]);
    assert!(!tree.nodes.contains_key(&5), "unreachable nodes go");
}

/// A hostile tree (a cycle, a huge list) ends, bounded.
#[test]
fn a_hostile_tree_is_bounded() {
    let mut nodes = vec![
        json!({"id": 1, "role": "rootWebArea", "child_ids": [2]}),
        json!({"id": 2, "role": "button", "child_ids": [1, 2], "offset_container_id": 2,
               "attributes": {"name": "x"}}),
    ];
    nodes.extend((3..10_000).map(|i| json!({"id": i, "role": "button"})));
    let mut tree = Tree::default();
    tree.apply(&json!({"updates": [{"root_id": 1, "nodes": nodes}]}));
    let told = tree.told();
    assert_eq!(told.len(), 1, "each node once: {told:?}");
}

/// A node naming more children than nodes are kept holds that many only.
#[test]
fn a_nodes_children_are_bounded() {
    let children: Vec<i64> = (2..(MAX_KEPT as i64 + 100)).collect();
    let mut tree = Tree::default();
    tree.apply(&json!({"updates": [{"root_id": 1, "nodes": [
        {"id": 1, "role": "rootWebArea", "child_ids": children}
    ]}]}));
    assert_eq!(tree.nodes[&1].children.len(), MAX_KEPT);
}

/// An update changing only what nodes say (a name, a value) is applied as is; one changing
/// the tree's shape lets what it left unreachable go.
#[test]
fn only_a_new_shape_looks_for_what_is_unreachable() {
    let mut tree = Tree::default();
    tree.apply(&page());
    tree.apply(&json!({"updates": [{"nodes": [
        {"id": 10, "role": "staticText", "offset_container_id": 1,
         "attributes": {"name": "Other text"}}
    ]}]}));
    let named: Vec<_> = tree.told().into_iter().map(|n| n.name).collect();
    assert!(named.contains(&"Other text".to_owned()));
    tree.apply(&json!({"updates": [{"nodes": [
        {"id": 2, "role": "genericContainer", "state": ["ignored"], "child_ids": [7],
         "offset_container_id": 1}
    ]}]}));
    assert!(!tree.nodes.contains_key(&6) && !tree.nodes.contains_key(&10), "unreachable: gone");
    assert!(tree.nodes.contains_key(&7));
}

/// A node moved by the layout alone is told where it is now (its container's place too).
#[test]
fn moved_nodes_take_their_new_places() {
    let mut tree = Tree::default();
    tree.apply(&page());
    tree.moved(&json!([
        {"id": 5, "new_location": {"bounds": {"x": 16.0, "y": 116.0, "width": 368.0, "height": 74.0},
                                   "offset_container_id": 1}},
        {"id": 99, "new_location": {"bounds": {"x": 1.0}}}
    ]));
    let heading = tree.told().into_iter().find(|n| n.id == 5).unwrap();
    assert_eq!(heading.rect, [16.0, 106.0, 368.0, 74.0], "less the page's scroll");
}

/// A node in a scaled and moved container (a CSS transform) is told where it shows: its
/// container's transform applied, as Chromium places it.
#[test]
fn a_transformed_container_moves_and_scales_what_is_in_it() {
    let mut tree = Tree::default();
    tree.apply(&json!({"updates": [{
        "root_id": 1,
        "nodes": [
            {"id": 1, "role": "rootWebArea", "child_ids": [7], "offset_container_id": 0,
             "location": {"x": 0.0, "y": 0.0, "width": 400.0, "height": 300.0}},
            {"id": 7, "role": "genericContainer", "state": ["ignored"], "child_ids": [8],
             "offset_container_id": 1,
             "location": {"x": 0.0, "y": 0.0, "width": 350.0, "height": 21.0},
             "transform": "[ 2 0 0 50\n  0 2 0 0\n  0 0 1 0\n  0 0 0 1 ]\n"},
            {"id": 8, "role": "button", "offset_container_id": 7,
             "location": {"x": 0.0, "y": 0.0, "width": 50.0, "height": 21.0},
             "attributes": {"name": "Press"}}
        ]
    }]}));
    let button = tree.told().into_iter().find(|n| n.id == 8).unwrap();
    assert_eq!(button.rect, [50.0, 0.0, 100.0, 42.0]);
    assert_eq!(Affine::parse("[ 1 0 0 NaN 0 1 0 0 0 0 1 0 0 0 0 1 ]"), None, "not finite");
    assert_eq!(Affine::parse("[ 1 0 ]"), None, "not a matrix");
}

/// A node's transform follows its moves: a location with one applies it, the next without
/// one leaves the node as it is (never the old transform kept).
#[test]
fn a_move_without_a_transform_drops_the_old_one() {
    let mut tree = Tree::default();
    tree.apply(&json!({"updates": [{
        "root_id": 1,
        "nodes": [
            {"id": 1, "role": "rootWebArea", "child_ids": [8], "offset_container_id": 0,
             "location": {"x": 0.0, "y": 0.0, "width": 400.0, "height": 300.0}},
            {"id": 8, "role": "button", "offset_container_id": 1,
             "location": {"x": 0.0, "y": 0.0, "width": 50.0, "height": 20.0},
             "attributes": {"name": "Press"}}
        ]
    }]}));
    let bounds = json!({"x": 0.0, "y": 0.0, "width": 50.0, "height": 20.0});
    tree.moved(&json!([{"id": 8, "new_location": {
        "bounds": bounds, "offset_container_id": 1,
        "transform": "[ 2 0 0 10\n 0 2 0 0\n 0 0 1 0\n 0 0 0 1 ]"
    }}]));
    let rect = |tree: &mut Tree| tree.told().into_iter().find(|n| n.id == 8).unwrap().rect;
    assert_eq!(rect(&mut tree), [10.0, 0.0, 100.0, 40.0], "scaled and moved");
    tree.moved(&json!([{"id": 8, "new_location": {"bounds": bounds, "offset_container_id": 1}}]));
    assert_eq!(rect(&mut tree), [0.0, 0.0, 50.0, 20.0], "as it is again");
}
