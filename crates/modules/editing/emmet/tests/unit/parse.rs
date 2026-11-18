use super::*;

#[test]
fn structure() {
    let nodes = parse("div.a.b#x[data-y=1 required]{Hi}").unwrap();
    let n = &nodes[0];
    assert_eq!(
        (n.name.as_str(), n.id.as_deref(), n.text.as_deref()),
        ("div", Some("x"), Some("Hi"))
    );
    assert_eq!(n.classes, ["a", "b"]);
    assert_eq!(
        n.attrs,
        [("data-y".to_owned(), Some("1".to_owned())), ("required".to_owned(), None)]
    );

    let nodes = parse("ul>li.item$*3>a").unwrap();
    assert_eq!(nodes[0].children.len(), 3);
    assert_eq!(nodes[0].children[2].classes, ["item3"]);
    assert_eq!(nodes[0].children[2].children[0].name, "a");

    // `^` climbs: `p` is a sibling of `div`.
    let nodes = parse("div>span^p").unwrap();
    assert_eq!(nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["div", "p"]);
    let nodes = parse("header+(nav>a)*2+footer").unwrap();
    assert_eq!(nodes.len(), 4);
    assert!(parse("div..x").is_none() && parse("(div").is_none() && parse("").is_none());
}
