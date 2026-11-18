use super::*;

#[test]
fn the_analysers_variants_win() {
    let t = tags(
        "/**\n * @return array\n * @psalm-return list<User>\n * @phpstan-return list<Admin>\n */",
    );
    assert_eq!(t.returns.as_deref(), Some("list<Admin>"));
    let t = tags("/**\n * @phpstan-param list<A> $a\n * @param array $a\n */");
    assert_eq!(t.params, [("a".into(), "list<A>".into())], "the plain tag after");
    let t = tags("/**\n * @phpstan-var Collection<int, User> $users\n * @var array $users\n */");
    assert_eq!(t.var, Some((Some("users".into()), "Collection<int, User>".into())));
}

#[test]
fn params_templates_and_parents() {
    let t = tags(
        "/**\n * @template TKey of array-key\n * @template-covariant TValue\n * @param callable(TValue, TKey): bool $filter keep\n * @param array<TKey, TValue> ...$items\n * @extends Base<TKey, TValue>\n * @implements \\IteratorAggregate<TKey, TValue>\n * @use HasItems<TValue>\n * @mixin \\Illuminate\\Database\\Eloquent\\Builder\n */",
    );
    assert_eq!(t.templates, [("TKey".into(), Some("array-key".into())), ("TValue".into(), None)]);
    assert_eq!(
        t.params,
        [
            ("filter".into(), "callable(TValue, TKey): bool".into()),
            ("items".into(), "array<TKey, TValue>".into())
        ]
    );
    assert_eq!(t.extends, ["Base<TKey, TValue>"]);
    assert_eq!(t.implements, ["\\IteratorAggregate<TKey, TValue>"]);
    assert_eq!(t.uses, ["HasItems<TValue>"]);
    assert_eq!(t.mixins, ["\\Illuminate\\Database\\Eloquent\\Builder"]);
}

#[test]
fn magic_members_with_where_their_names_are() {
    let doc = "/**\n * @property string $nickname\n * @property-read int $age\n * @method bool isAdmin()\n * @method static \\Illuminate\\Database\\Query\\Builder table(string $table, string|null $as = null)\n * @method static void flush()\n * @method foo(int $a, array $b = [])\n */";
    let t = tags(doc);
    let names: Vec<(&str, &str, bool, bool)> = t
        .magic
        .iter()
        .map(|m| (m.name.as_str(), m.detail.as_str(), m.is_method, m.is_static))
        .collect();
    assert_eq!(
        names,
        [
            ("nickname", "string", false, false),
            ("age", "int", false, false),
            ("isAdmin", "(): bool", true, false),
            (
                "table",
                "(string $table, string|null $as = null): \\Illuminate\\Database\\Query\\Builder",
                true,
                true
            ),
            ("flush", "(): void", true, true),
            ("foo", "(int $a, array $b = [])", true, false),
        ]
    );
    // A property's offset is its `$`, as the code's are.
    for m in &t.magic {
        let at = if m.is_method { m.name.clone() } else { format!("${}", m.name) };
        assert!(doc[m.offset..].starts_with(&at), "{} at its offset", m.name);
    }
    assert_eq!(t.magic[1].tag, "@property-read");
}

#[test]
fn deprecation() {
    assert!(tags("/** @deprecated use modern() */").deprecated);
    assert!(!tags("/** Not @deprecated here */").deprecated, "only as a tag");
}

#[test]
fn facts_round_trip() {
    let facts = Facts {
        returns: Some("Collection<int, User>".into()),
        var: None,
        params: vec![("callback".into(), "callable(TValue): TMapped".into())],
        templates: vec![("TMapped".into(), None), ("T".into(), Some("Model".into()))],
        extends: vec!["Base<int>".into()],
        mixins: vec!["Builder".into()],
        deprecated: true,
        backed: Some("string".into()),
    };
    let encoded = facts.encode();
    assert_eq!(Facts::decode(encoded.as_deref()), facts);
    assert!(Facts::says_deprecated(encoded.as_deref()));
    assert_eq!(Facts::default().encode(), None, "nothing to say: nothing kept");
}
