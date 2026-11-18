use super::*;

fn show(text: &str) -> String {
    parse(text, &NameScope::GLOBAL).map(|t| t.show()).unwrap_or_default()
}

fn templated(text: &str, templates: &[&str]) -> Ty {
    let names: Vec<Box<str>> = templates.iter().map(|t| (*t).into()).collect();
    let scope = NameScope { file: None, at: 0, templates: &names };
    parse(text, &scope).unwrap_or(Ty::Mixed)
}

#[test]
fn plain_and_nullable_types() {
    assert_eq!(show("User"), "User");
    assert_eq!(show("?User"), "?User");
    assert_eq!(show("User|null"), "?User");
    assert_eq!(show("\\App\\Model\\User"), "User");
    assert_eq!(
        parse("\\App\\Model\\User", &NameScope::GLOBAL),
        Some(Ty::named("App\\Model\\User"))
    );
    assert_eq!(show("int|string"), "int|string");
    assert_eq!(show("A&B"), "A|B", "an intersection offers both");
    assert_eq!(parse("", &NameScope::GLOBAL), None);
}

#[test]
fn generics_arrays_and_lists() {
    assert_eq!(show("Collection<int, User>"), "Collection<int, User>");
    assert_eq!(
        parse("list<User>", &NameScope::GLOBAL).map(|t| t.element()),
        Some(Ty::named("User"))
    );
    assert_eq!(parse("User[]", &NameScope::GLOBAL).map(|t| t.element()), Some(Ty::named("User")));
    assert_eq!(
        parse("array<string, User>", &NameScope::GLOBAL)
            .map(|t| (t.key().show(), t.element().show())),
        Some(("string".into(), "User".into()))
    );
    assert_eq!(
        parse("iterable<int, User>", &NameScope::GLOBAL).map(|t| t.element()),
        Some(Ty::named("User"))
    );
    assert_eq!(
        parse("non-empty-list<User>", &NameScope::GLOBAL).map(|t| t.element()),
        Some(Ty::named("User"))
    );
    assert_eq!(show("(A|B)[]"), "array<A|B>");
    assert_eq!(show("int<0, max>"), "int");
}

#[test]
fn shapes_and_their_keys() {
    let ty =
        parse("array{id: int, 'name'?: string, address: Address}", &NameScope::GLOBAL).unwrap();
    assert_eq!(ty.at_key(Some("address")), Ty::named("Address"));
    assert_eq!(ty.at_key(Some("name")).show(), "string");
    let list = parse("array{User, Address}", &NameScope::GLOBAL).unwrap();
    assert_eq!(list.at_key(Some("1")), Ty::named("Address"));
}

#[test]
fn templates_static_self_and_class_strings() {
    assert_eq!(
        templated("TValue|null", &["TValue"]),
        Ty::union(Ty::Template("TValue".into()), Ty::null())
    );
    assert_eq!(templated("static<TKey, TMapped>", &["TKey"]).show(), "static<TKey, TMapped>");
    assert_eq!(templated("$this", &[]), Ty::Static(Vec::new()));
    assert_eq!(templated("self", &[]), Ty::SelfType);
    assert_eq!(
        templated("class-string<T>", &["T"]),
        Ty::ClassString(Box::new(Ty::Template("T".into())))
    );
}

#[test]
fn callables_conditionals_and_oddities() {
    let callable = templated("callable(TValue, TKey): TMapValue", &["TValue", "TKey", "TMapValue"]);
    let Ty::Callable(params, ret) = callable else { panic!("a callable") };
    assert_eq!(params, [Ty::Template("TValue".into()), Ty::Template("TKey".into())]);
    assert_eq!(*ret, Ty::Template("TMapValue".into()));
    assert!(matches!(templated("Closure(int $a): void", &[]), Ty::Callable(..)));
    let conditional = show("($type is class-string<Flow> ? FlowForm : Form)");
    assert_eq!(conditional, "FlowForm|Form");
    // Literals and constants: not classes.
    assert_eq!(show("'a'|'b'"), "string");
    assert_eq!(show("Foo::BAR"), "mixed");
    assert_eq!(show("key-of<T>"), "mixed");
    // Nonsense never fails.
    assert_eq!(show("<<<>>>"), "mixed");
    assert!(parse(&"(".repeat(200), &NameScope::GLOBAL).is_some());
}

#[test]
fn unions_merge_and_lose_null() {
    let u = Ty::union(Ty::named("A"), Ty::union(Ty::named("B"), Ty::named("A")));
    assert_eq!(u.show(), "A|B");
    assert_eq!(Ty::union(Ty::named("A"), Ty::null()).without_null(), Ty::named("A"));
    assert_eq!(Ty::union(Ty::Mixed, Ty::named("A")), Ty::named("A"));
    assert_eq!(u.class(), Some("A"));
}

#[test]
fn doc_types_end_where_the_type_does() {
    assert_eq!(
        split_doc_type("array<int, User> $users the list"),
        ("array<int, User>", " $users the list")
    );
    assert_eq!(split_doc_type("int | string $x"), ("int | string", " $x"));
    assert_eq!(split_doc_type("callable(int): bool $f"), ("callable(int): bool", " $f"));
    assert_eq!(
        split_doc_type("array{id: int, name: string} $row"),
        ("array{id: int, name: string}", " $row")
    );
    assert_eq!(split_doc_type("User"), ("User", ""));
}
