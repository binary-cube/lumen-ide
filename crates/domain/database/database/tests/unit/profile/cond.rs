use super::*;
use crate::profile::node::ObjectKind;

const PG: Flavor = Flavor::Postgres { version: Some(170_000) };

fn table(name: &str, alias: Option<&str>) -> ObjectRef {
    ObjectRef {
        kind: ObjectKind::Table,
        schema: None,
        name: name.into(),
        alias: alias.map(str::to_owned),
    }
}

#[test]
fn postgres_casts_its_varchar_columns() {
    let text = "((status)::text = 'paid'::text)";
    let c = parse(text, PG);
    assert_eq!(c.predicates.len(), 1);
    let p = &c.predicates[0];
    assert!(p.column.is("status") && p.column.qualifier.is_none());
    assert_eq!(p.compare, Compare::Eq);
    assert_eq!(p.cast.as_deref(), Some("text"));
    assert!(p.literal && p.wrapper.is_none() && !p.is_plain() && !p.in_or);
    assert_eq!(&text[p.range.clone()], "(status)::text = 'paid'::text");
    assert_eq!(&text[p.column.range.clone()], "status");
}

#[test]
fn any_of_an_array_is_in() {
    let c = parse("(status = ANY ('{new,paid}'::text[]))", PG);
    assert_eq!(c.predicates[0].compare, Compare::In);
    assert!(c.predicates[0].is_plain() && c.predicates[0].literal);
    let c = parse("(id IN (1, 2, 3))", PG);
    assert_eq!(c.predicates[0].compare, Compare::In);
    let c = parse("(id NOT IN (1, 2))", PG);
    assert_eq!(c.predicates[0].compare, Compare::Not);
}

#[test]
fn a_function_around_a_column_is_its_wrapper() {
    let text = "(lower((email)::text) = 'a@b.ro'::text)";
    let c = parse(text, PG);
    let p = &c.predicates[0];
    assert!(p.column.is("email"));
    let w = p.wrapper.as_ref().unwrap();
    assert_eq!(w.function, "lower");
    assert!(w.single);
    assert_eq!(&text[w.range.clone()], "lower((email)::text)");
    assert_eq!(p.cast.as_deref(), Some("text"), "the cast inside the call");
    // A function of values is a value: the column stays plain.
    let c = parse("(created_at > (now() - '1 day'::interval))", PG);
    assert!(c.predicates.is_empty(), "an expression of values isn't read as one");
    let c = parse("(created_at > now())", PG);
    assert_eq!(c.predicates[0].compare, Compare::Range);
    assert!(c.predicates[0].is_plain() && !c.predicates[0].literal);
    // Two columns in one call: neither is searched by it.
    assert!(parse("(coalesce(a, b) = 1)", PG).predicates.is_empty());
}

#[test]
fn ranges_between_and_null_tests() {
    let c = parse("((total >= 10) AND (total < 100) AND (note IS NULL))", PG);
    let kinds: Vec<Compare> = c.predicates.iter().map(|p| p.compare).collect();
    assert_eq!(kinds, [Compare::Range, Compare::Range, Compare::IsNull]);
    let c = parse("a BETWEEN 1 AND 5 AND b = 2", Flavor::Sqlite);
    assert_eq!(c.predicates.len(), 2, "BETWEEN's AND isn't a conjunction");
    assert_eq!(c.predicates[0].compare, Compare::Range);
    assert!(c.predicates[1].column.is("b"));
    let c = parse("(note IS NOT NULL)", PG);
    assert_eq!(c.predicates[0].compare, Compare::IsNull);
    let c = parse("(a <> 3)", PG);
    assert_eq!(c.predicates[0].compare, Compare::Not);
}

#[test]
fn ors_at_the_top_are_branches() {
    let c = parse("((a = 1) OR (b = 2))", PG);
    assert_eq!(c.or_branches.len(), 2);
    assert!(c.or_branches[0][0].is("a") && c.or_branches[1][0].is("b"));
    assert!(c.predicates.iter().all(|p| p.in_or));
    let t = table("t", None);
    assert_eq!(c.of_object(&t).count(), 0, "an OR alone selects nothing");
    // The same column on both sides.
    let c = parse("((a = 1) OR (a = 2))", PG);
    assert!(c.or_branches.iter().all(|b| b.iter().all(|col| col.is("a"))));
    // Under an AND, the other part still decides.
    let c = parse("((x = 1) AND ((a = 1) OR (b = 2)))", PG);
    assert_eq!(c.of_object(&t).count(), 1);
    assert_eq!(c.or_branches.len(), 2);
}

#[test]
fn like_with_a_leading_wildcard() {
    let c = parse("((name)::text ~~ '%smith'::text)", PG);
    assert_eq!(c.predicates[0].compare, Compare::Like);
    assert!(c.predicates[0].leading_wildcard);
    let c = parse("((name)::text ~~ 'smith%'::text)", PG);
    assert!(!c.predicates[0].leading_wildcard);
    let c = parse("name LIKE '%x%'", Flavor::Sqlite);
    assert!(c.predicates[0].leading_wildcard);
    let c = parse("name NOT LIKE '%x'", Flavor::Sqlite);
    assert_eq!(c.predicates[0].compare, Compare::Not);
    assert!(!c.predicates[0].leading_wildcard);
}

#[test]
fn join_pairs_and_their_sides() {
    let c = parse("(o.customer_id = c.id)", PG);
    assert_eq!(c.joins.len(), 1);
    let pair = &c.joins[0];
    let orders = table("orders", Some("o"));
    let (mine, other) = pair.side_of(&orders).unwrap();
    assert!(mine.is("customer_id") && other.is("id"));
    assert_eq!(other.qualifier.as_deref(), Some("c"));
    // A pair of unqualified columns could be either table's.
    let c = parse("(customer_id = id)", PG);
    assert!(c.joins[0].side_of(&orders).is_none());
    // An outer reference in an index condition (`$1` of a subplan is a value).
    let c = parse("(customer_id = $1)", PG);
    assert!(!c.predicates[0].literal);
}

#[test]
fn columns_belong_to_their_table_by_alias_or_name() {
    let orders = table("orders", Some("o"));
    let c =
        parse("((o.status = 'x') AND (c.region = 'eu') AND (orders.total > 1) AND (id = 1))", PG);
    let mine: Vec<&str> = c.of_object(&orders).map(|p| p.column.name.as_str()).collect();
    assert_eq!(mine, ["status", "total", "id"]);
    let qualified = &c.predicates[0].column;
    assert_eq!(qualified.qualifier.as_deref(), Some("o"));
}

#[test]
fn sqlite_conditions_and_parameters() {
    let c = parse("(a=? AND b>?)", Flavor::Sqlite);
    assert_eq!(c.predicates.len(), 2);
    assert_eq!((c.predicates[0].compare, c.predicates[1].compare), (Compare::Eq, Compare::Range));
    assert!(!c.predicates[0].literal, "a parameter");
    let c = parse("customer_id=?", Flavor::Sqlite);
    assert!(c.predicates[0].column.is("customer_id"));
    // A cast written by hand, and a typed literal.
    let c = parse("CAST(code AS TEXT) = '7' AND day > DATE '2024-01-01'", Flavor::Generic);
    assert_eq!(c.predicates[0].cast.as_deref(), Some("text"));
    assert!(c.predicates[1].literal && c.predicates[1].is_plain());
}

#[test]
fn what_isnt_a_comparison_is_only_mentioned() {
    let c = parse("((SubPlan 1) > 2)", PG);
    assert!(c.predicates.is_empty() && c.columns.is_empty(), "a subplan's output isn't a column");
    let c = parse("(NOT active)", PG);
    assert!(c.predicates.is_empty());
    assert!(c.columns[0].is("active"));
    let c = parse("((a + b) > 10)", PG);
    assert!(c.predicates.is_empty());
    assert_eq!(c.columns.len(), 2);
    assert!(parse("", PG).predicates.is_empty());
    assert!(parse("(((", PG).predicates.is_empty(), "never a panic on a broken text");
}
