use super::*;
use crate::sql::lex;

const PG: Flavor = Flavor::Postgres { version: None };
const LITE: Flavor = Flavor::Sqlite;

fn shape(sql: &str, flavor: Flavor) -> Result<QueryShape, NotEditable> {
    query_shape(sql, &lex(sql, flavor), flavor)
}

fn items(sql: &str, flavor: Flavor) -> Vec<SelectItem> {
    shape(sql, flavor).map(|s| s.items).unwrap_or_else(|e| panic!("{sql}: {e:?}"))
}

fn col(name: &str) -> SelectItem {
    SelectItem::Column(name.to_owned())
}

#[test]
fn a_query_of_one_table_is_told_item_by_item() {
    assert_eq!(items("SELECT * FROM users", LITE), [SelectItem::All]);
    assert_eq!(items("select u.* from users u where u.id > 3", LITE), [SelectItem::All]);
    assert_eq!(items("SELECT users.* FROM main.users", LITE), [SelectItem::All]);
    assert_eq!(
        items("SELECT id, u.name AS who, email mail, price * 2, 'x', NULL FROM users AS u", LITE),
        [
            col("id"),
            col("name"),
            col("email"),
            SelectItem::Expression,
            SelectItem::Expression,
            SelectItem::Expression,
        ]
    );
    assert_eq!(
        items("SELECT lower(name), coalesce(a, b) AS c FROM t ORDER BY 1 LIMIT 10", LITE),
        [SelectItem::Expression, SelectItem::Expression]
    );
    // Another table's qualifier isn't this one's column.
    assert_eq!(items("SELECT x.id FROM users u", LITE), [SelectItem::Expression]);
    assert_eq!(items("SELECT ALL id FROM t", LITE), [col("id")]);
    assert_eq!(items("SELECT rowid, * FROM t", LITE), [col("rowid"), SelectItem::All]);
}

#[test]
fn names_are_read_as_the_engine_reads_them() {
    let s = shape("SELECT Id, \"Name\" FROM Public.Users U", PG).unwrap();
    assert_eq!(s.table, TableRef::in_schema("public", "users"));
    assert_eq!(s.alias.as_deref(), Some("U"));
    assert_eq!(s.items, [col("id"), col("Name")]);
    assert_eq!(items("SELECT [Name] FROM t", LITE), [col("Name")]);
    assert_eq!(items("SELECT u.Name FROM Users u", LITE), [col("Name")]);
}

#[test]
fn what_isn_t_one_table_s_rows_is_refused_with_its_reason() {
    for (sql, why) in [
        ("SELECT DISTINCT name FROM users", NotEditable::Distinct),
        ("SELECT name FROM users GROUP BY name", NotEditable::Grouped),
        ("SELECT name FROM users HAVING count(*) > 1", NotEditable::Grouped),
        ("SELECT count(*) FROM users", NotEditable::Aggregate("count".into())),
        ("SELECT id, MAX(total) FROM orders", NotEditable::Aggregate("max".into())),
        (
            "SELECT id, row_number() OVER (ORDER BY id) FROM t",
            NotEditable::Aggregate("over".into()),
        ),
        ("SELECT * FROM a JOIN b ON a.id = b.a_id", NotEditable::NotOneTable),
        ("SELECT * FROM a, b", NotEditable::NotOneTable),
        ("SELECT * FROM (SELECT 1) t", NotEditable::NotOneTable),
        ("SELECT * FROM a UNION SELECT * FROM b", NotEditable::NotOneTable),
        ("WITH x AS (SELECT 1) SELECT * FROM x", NotEditable::NotQuery),
        ("SELECT 1", NotEditable::NotOneTable),
        ("UPDATE t SET a = 1 RETURNING *", NotEditable::NotQuery),
        ("VALUES (1)", NotEditable::NotQuery),
        ("SELECT * FROM t FOR UPDATE", NotEditable::Locking),
        ("", NotEditable::NotQuery),
    ] {
        assert_eq!(shape(sql, LITE), Err(why), "{sql}");
    }
    // An aggregate's name as a column isn't one.
    assert_eq!(items("SELECT count, total FROM stats", LITE), [col("count"), col("total")]);
    // A subquery in the select list or the filter keeps the rows the table's.
    assert_eq!(items("SELECT id FROM t WHERE id IN (SELECT t_id FROM u)", LITE), [col("id")],);
}
