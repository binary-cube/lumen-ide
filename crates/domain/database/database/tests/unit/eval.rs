use std::cmp::Ordering;

use super::*;

fn text(t: &str) -> Value {
    Value::Text(t.to_owned())
}

#[test]
fn values_sort_nulls_first_then_numbers_by_value_then_text() {
    assert_eq!(compare(&Value::Null, &Value::Int(1)), Ordering::Less);
    assert_eq!(compare(&Value::Int(9), &Value::Int(10)), Ordering::Less);
    assert_eq!(compare(&Value::Other("9".into()), &Value::Other("10".into())), Ordering::Less);
    assert_eq!(compare(&Value::Other("2,5".into()), &Value::Float(2.4)), Ordering::Greater);
    assert_eq!(compare(&Value::Int(5), &text("abc")), Ordering::Less);
    assert_eq!(compare(&text("b"), &text("a")), Ordering::Greater);
    assert_eq!(number(&text("nan")), None);
    assert_eq!(number(&text("1e3")), Some(1000.0));

    let order = vec![
        Order { column: "team".into(), descending: false },
        Order { column: "score".into(), descending: true },
    ];
    let a = [text("red"), Value::Int(10)];
    let b = [text("red"), Value::Int(20)];
    assert_eq!(compare_rows(&order, &a, &b), Ordering::Greater, "score descending");
    assert_eq!(compare_rows(&order, &[text("blue"), Value::Int(1)], &b), Ordering::Less);
}

#[test]
fn filters_answer_as_sql_does() {
    let row = |column: &str| match column {
        "name" => Some(text("Ana Maria")),
        "age" => Some(Value::Other("31".into())),
        "note" => Some(Value::Null),
        _ => None,
    };
    let compare = |column: &str, op, value| Filter::Compare { column: column.into(), op, value };
    assert!(matches(&compare("age", CompareOp::Greater, Value::Int(30)), &row));
    assert!(matches(&compare("age", CompareOp::Eq, Value::Int(31)), &row));
    assert!(!matches(&compare("age", CompareOp::Less, Value::Int(31)), &row));
    assert!(matches(&compare("name", CompareOp::Eq, text("Ana Maria")), &row));
    // NULL is never compared equal, nor unequal.
    assert!(!matches(&compare("note", CompareOp::Eq, text("")), &row));
    assert!(!matches(&compare("note", CompareOp::NotEq, text("x")), &row));
    assert!(matches(&Filter::IsNull("note".into()), &row));
    assert!(matches(&Filter::IsNull("missing".into()), &row));
    assert!(matches(&Filter::Contains { column: "name".into(), text: "MARIA".into() }, &row));
    let both = Filter::And(vec![
        Filter::NotNull("name".into()),
        Filter::Not(Box::new(compare("age", CompareOp::Less, Value::Int(18)))),
    ]);
    assert!(matches(&both, &row));
    assert!(!matches(&Filter::Or(vec![Filter::IsNull("name".into())]), &row));
    assert_eq!(
        filter_columns(&Filter::Or(vec![both, Filter::IsNull("note".into())])),
        ["name", "age", "note"]
    );
}
