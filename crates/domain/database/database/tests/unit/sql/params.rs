use super::*;
use crate::dialect::GenericSql;
use crate::postgres::Postgres;
use crate::sql::{EnginePosition, TextPosition, lex, locate};
use crate::sqlite::Sqlite;

const PG: Flavor = Flavor::Postgres { version: None };

fn found(text: &str, flavor: Flavor) -> Vec<Parameter> {
    params(text, &lex(text, flavor), &ParamStyles::of(flavor))
}

fn names(text: &str, flavor: Flavor) -> Vec<ParamName> {
    found(text, flavor).into_iter().map(|p| p.name).collect()
}

fn named(n: &str) -> ParamName {
    ParamName::Named(n.to_owned())
}

#[test]
fn parameters_of_every_form() {
    use ParamName::*;
    assert_eq!(names("select ? , ?", Flavor::Sqlite), [Positional(1), Positional(2)]);
    assert_eq!(names("select ?2, ?1", Flavor::Sqlite), [Numbered(2), Numbered(1)]);
    assert_eq!(names("select $1, $2", PG), [Numbered(1), Numbered(2)]);
    assert_eq!(
        names("select :id, @name, $x", Flavor::Sqlite),
        [named("id"), named("name"), named("x")]
    );
    assert_eq!(names("select :id", PG), [named("id")]);
    assert_eq!(names("select ?", Flavor::Generic), [Positional(1)]);
    assert_eq!(names("select ?, :a", Flavor::MySql), [Positional(1), named("a")]);
}

#[test]
fn each_flavor_reads_only_its_own() {
    // PostgreSQL's `?` is a JSON operator; MySQL's `@x` a user variable.
    assert!(names("select data ? 'k' from t", PG).is_empty());
    assert!(names("select @total := 1", Flavor::MySql).is_empty());
    assert!(names("select a::int, x := 1", PG).is_empty());
    // Every style asked for.
    let text = "select ?, @a";
    assert_eq!(
        params(text, &lex(text, PG), &ParamStyles::all()).len(),
        1,
        "`@` is an operator in PostgreSQL"
    );
    assert_eq!(params(text, &lex(text, Flavor::Sqlite), &ParamStyles::all()).len(), 2);
}

#[test]
fn a_parameter_written_twice_is_one_with_two_places() {
    let text = "select * from t where a = :id or b = :id and c = $1 and d = $1";
    let p = found(text, PG);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].name, named("id"));
    assert_eq!(p[0].places, [26..29, 37..40]);
    assert_eq!(p[1].places.len(), 2);
    // `:id` and `@id` are the same name.
    assert_eq!(found("select :id, @id", Flavor::Sqlite).len(), 1);
    // Every `?` is its own.
    assert_eq!(found("select ?, ?, ?", Flavor::Sqlite).len(), 3);
}

#[test]
fn no_parameters_in_strings_comments_names_or_bodies() {
    let pg = "select ':x $1', \"$2\", $$ :y $3 $$, $f$ $4 $f$ -- :z $5\n/* $6 */ from t";
    assert!(found(pg, PG).is_empty());
    let sqlite = "select '?', [?], `:a`, \"@b\" -- ?\n";
    assert!(found(sqlite, Flavor::Sqlite).is_empty());
}

fn values(pairs: &[(ParamName, ParamValue)]) -> HashMap<ParamName, ParamValue> {
    pairs.iter().cloned().collect()
}

#[test]
fn values_are_written_in_as_the_dialects_literals() {
    let text = "select * from t where id = :id and name = :name and ok = :ok";
    let p = found(text, Flavor::Sqlite);
    let vals = values(&[
        (named("id"), ParamValue::Literal(Value::Int(7))),
        (named("name"), ParamValue::Literal(Value::Text("O'Brien; DROP TABLE t; --".into()))),
        (named("ok"), ParamValue::Literal(Value::Bool(true))),
    ]);
    let bound = bind(text, 0..text.len(), &p, &vals, &Sqlite).unwrap();
    assert_eq!(
        bound.text,
        "select * from t where id = 7 and name = 'O''Brien; DROP TABLE t; --' and ok = 1"
    );
    // PostgreSQL writes its own literals (a backslash: an escape string).
    let text = "select $1, $2";
    let p = found(text, PG);
    let vals = values(&[
        (ParamName::Numbered(1), ParamValue::Literal(Value::Text("a\\b".into()))),
        (ParamName::Numbered(2), ParamValue::Literal(Value::Null)),
    ]);
    let bound = bind(text, 0..text.len(), &p, &vals, &Postgres::default()).unwrap();
    assert_eq!(bound.text, "select E'a\\\\b', NULL");
}

#[test]
fn a_raw_value_is_written_as_typed_and_repeats_get_the_same() {
    let text = "select :t, :t, ?";
    let p = found(text, Flavor::Sqlite);
    let vals = values(&[
        (named("t"), ParamValue::Raw("now()".into())),
        (ParamName::Positional(1), ParamValue::Literal(Value::Float(1.5))),
    ]);
    let bound = bind(text, 0..text.len(), &p, &vals, &GenericSql).unwrap();
    assert_eq!(bound.text, "select now(), now(), 1.5");
}

#[test]
fn a_missing_value_is_an_error_never_a_null() {
    let text = "select :a, :b";
    let p = found(text, Flavor::Sqlite);
    let vals = values(&[(named("a"), ParamValue::Literal(Value::Int(1)))]);
    assert_eq!(bind(text, 0..text.len(), &p, &vals, &Sqlite), Err(BindError::Missing(named("b"))));
    // A value the dialect can't write.
    let vals = values(&[
        (named("a"), ParamValue::Literal(Value::Float(f64::NAN))),
        (named("b"), ParamValue::Literal(Value::Int(1))),
    ]);
    let refused = bind(text, 0..text.len(), &p, &vals, &Sqlite);
    assert!(matches!(&refused, Err(BindError::Refused(n, _)) if *n == named("a")), "{refused:?}");
}

#[test]
fn only_the_statements_own_range_is_bound() {
    let doc = "select :a;\nselect :a + :b;";
    let p = found(doc, Flavor::Sqlite);
    let second = 11..doc.len() - 1;
    let vals = values(&[
        (named("a"), ParamValue::Literal(Value::Int(1))),
        (named("b"), ParamValue::Literal(Value::Int(22))),
    ]);
    let bound = bind(doc, second, &p, &vals, &Sqlite).unwrap();
    assert_eq!(bound.text, "select 1 + 22");
    assert_eq!(bound.map.start, 11);
}

#[test]
fn the_map_takes_positions_back_to_the_document() {
    let doc = "-- x\nselect :long_name + 'ș' + :b from nowhere";
    let range = 5..doc.len();
    let p = found(doc, Flavor::Sqlite);
    let vals = values(&[
        (named("long_name"), ParamValue::Literal(Value::Int(1))),
        (named("b"), ParamValue::Literal(Value::Text("a much longer value".into()))),
    ]);
    let bound = bind(doc, range, &p, &vals, &Sqlite).unwrap();
    assert_eq!(bound.text, "select 1 + 'ș' + 'a much longer value' from nowhere");
    let map = &bound.map;
    // Before any value: the same place.
    assert_eq!(map.to_document(0), 5);
    // In a value: where its parameter starts.
    let sent_b = bound.text.find("'a much").unwrap();
    assert_eq!(map.to_document(sent_b + 3), doc.find(":b").unwrap());
    assert_eq!(map.to_document(bound.text.find('1').unwrap()), doc.find(":long").unwrap());
    // After it: shifted back.
    let sent_from = bound.text.find("from").unwrap();
    assert_eq!(map.to_document(sent_from), doc.find("from").unwrap());
    assert_eq!(map.to_document(bound.text.len()), doc.len());
    // An engine's error at `nowhere` lands there in the document.
    let chars = bound.text[..bound.text.find("nowhere").unwrap()].chars().count() + 1;
    let at = locate(doc, &bound.text, map, EnginePosition::Character(chars)).unwrap();
    assert_eq!(at, TextPosition::of(doc, doc.find("nowhere").unwrap()));
    assert_eq!((at.line, at.column), (1, doc.lines().nth(1).unwrap().find("nowhere").unwrap() - 1));
}

#[test]
fn a_plain_map_only_shifts_by_the_start() {
    let map = BindMap::plain(40);
    assert_eq!(map.to_document(0), 40);
    assert_eq!(map.to_document(7), 47);
}
