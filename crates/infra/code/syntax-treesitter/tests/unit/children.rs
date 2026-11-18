use super::*;

fn tree(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    parser.parse(source, None).unwrap()
}

/// The cursor's children are `child(i)`'s, in the same order (named ones too); pushed on a
/// stack they come off first to last.
#[test]
fn the_children_are_child_i_s_in_order() {
    let source = format!("[{}]", (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join(", "));
    let tree = tree(&source);
    let array = tree.root_node().child(0).and_then(|s| s.child(0)).unwrap();
    assert_eq!(array.kind(), "array");
    let by_index: Vec<_> = (0..array.child_count()).filter_map(|i| array.child(i)).collect();
    assert_eq!(children(array).collect::<Vec<_>>(), by_index);
    let named: Vec<_> =
        (0..array.named_child_count() as u32).filter_map(|i| array.named_child(i)).collect();
    assert_eq!(named_children(array).collect::<Vec<_>>(), named);
    assert_eq!(named.len(), 500);
    let mut stack = vec![tree.root_node()];
    push_children(&mut stack, array);
    let popped: Vec<_> = std::iter::from_fn(|| stack.pop()).take(by_index.len()).collect();
    assert_eq!(popped, by_index);
    let leaf = named[0];
    assert_eq!(children(leaf).count(), 0);
}
