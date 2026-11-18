use super::*;
use crate::data_grid::source::CellClass;

fn columns() -> Vec<GridColumn> {
    ["id", "name", "Order", "price"]
        .iter()
        .map(|n| {
            GridColumn::new(*n, "", if *n == "name" { CellClass::Text } else { CellClass::Integer })
        })
        .collect()
}

fn parse(text: &str) -> Result<Option<GridFilter>, ParseError> {
    parse_where(text, &columns())
}

fn cmp(column: &str, op: Compare, value: CellValue) -> GridFilter {
    GridFilter::Compare { column: column.into(), op, value }
}

#[test]
fn comparisons_read_values_as_values() {
    assert_eq!(parse("id = 3"), Ok(Some(cmp("id", Compare::Eq, CellValue::Int(3)))));
    assert_eq!(
        parse("price >= -2.5"),
        Ok(Some(cmp("price", Compare::GreaterEq, CellValue::Float(-2.5))))
    );
    assert_eq!(
        parse("name <> 'O''Brien'"),
        Ok(Some(cmp("name", Compare::NotEq, CellValue::Text("O'Brien".into()))))
    );
    assert_eq!(
        parse("name != 'x'"),
        Ok(Some(cmp("name", Compare::NotEq, CellValue::Text("x".into()))))
    );
    assert_eq!(parse("id == true"), Ok(Some(cmp("id", Compare::Eq, CellValue::Bool(true)))));
    assert_eq!(parse("  "), Ok(None));
}

#[test]
fn names_match_any_case_and_quoted_ones_are_never_keywords() {
    // The source's spelling is kept.
    assert_eq!(parse("ID < 1"), Ok(Some(cmp("id", Compare::Less, CellValue::Int(1)))));
    assert_eq!(parse("\"Order\" > 1"), Ok(Some(cmp("Order", Compare::Greater, CellValue::Int(1)))));
    assert_eq!(parse("`order` <= 1"), Ok(Some(cmp("Order", Compare::LessEq, CellValue::Int(1)))));
    assert_eq!(parse("[Order] = 1"), Ok(Some(cmp("Order", Compare::Eq, CellValue::Int(1)))));
    assert_eq!(parse("nope = 1"), Err(ParseError::UnknownColumn("nope".into())));
}

#[test]
fn and_binds_tighter_than_or_and_parentheses_group() {
    let a = cmp("id", Compare::Eq, CellValue::Int(1));
    let b = cmp("id", Compare::Eq, CellValue::Int(2));
    let c = GridFilter::IsNull("name".into());
    assert_eq!(
        parse("id = 1 OR id = 2 and name is null"),
        Ok(Some(GridFilter::Or(vec![a.clone(), GridFilter::And(vec![b.clone(), c.clone()])])))
    );
    assert_eq!(
        parse("(id = 1 or id = 2) AND name IS NULL"),
        Ok(Some(GridFilter::And(vec![GridFilter::Or(vec![a.clone(), b]), c])))
    );
    assert_eq!(parse("NOT id = 1"), Ok(Some(GridFilter::Not(Box::new(a)))));
    assert_eq!(parse("name is not null"), Ok(Some(GridFilter::NotNull("name".into()))));
}

#[test]
fn like_holds_a_text_or_is_one() {
    let holds = GridFilter::Contains { column: "name".into(), text: "ann".into() };
    assert_eq!(parse("name LIKE '%ann%'"), Ok(Some(holds.clone())));
    assert_eq!(parse("name not like '%ann%'"), Ok(Some(GridFilter::Not(Box::new(holds)))));
    assert_eq!(
        parse("name like 'ann'"),
        Ok(Some(cmp("name", Compare::Eq, CellValue::Text("ann".into()))))
    );
    assert_eq!(parse("name like 'a%n'"), Err(ParseError::Pattern("a%n".into())));
}

#[test]
fn what_isnt_understood_says_where() {
    assert_eq!(parse("id = NULL"), Err(ParseError::NullCompare));
    assert_eq!(parse("name = 'x"), Err(ParseError::Unterminated));
    assert_eq!(parse("id = 1 id"), Err(ParseError::Unexpected { found: Some("id".into()) }));
    assert_eq!(parse("id ="), Err(ParseError::Unexpected { found: None }));
    assert_eq!(parse("(id = 1"), Err(ParseError::Unexpected { found: None }));
    assert_eq!(parse("id ; 1"), Err(ParseError::Unexpected { found: Some(";".into()) }));
}

#[test]
fn a_filter_written_back_reads_the_same() {
    let filters = [
        "id = 1 AND (name = 'a' OR name IS NULL)",
        "NOT name LIKE '%x''y%'",
        "\"Order\" <> 2.5 OR price IS NOT NULL",
    ];
    for text in filters {
        let filter = parse(text).unwrap().unwrap();
        let written = where_text(&filter);
        assert_eq!(parse(&written).unwrap().unwrap(), filter, "{written}");
    }
    assert_eq!(where_text(&parse("id=1 and name='a'").unwrap().unwrap()), "id = 1 AND name = 'a'");
}

#[test]
fn quick_filters_are_added_with_and() {
    let a = cmp("id", Compare::Eq, CellValue::Int(1));
    let b = GridFilter::IsNull("name".into());
    let both = a.clone().and(b.clone());
    assert_eq!(both, GridFilter::And(vec![a.clone(), b.clone()]));
    let three = both.and(GridFilter::NotNull("price".into()));
    assert_eq!(three, GridFilter::And(vec![a, b, GridFilter::NotNull("price".into())]));
}

#[test]
fn order_by_reads_columns_and_directions() {
    let keys = parse_order("name desc, ID, price ASC, name", &columns()).unwrap();
    let key = |c: &str, d| SortKey { column: c.into(), descending: d };
    assert_eq!(keys, vec![key("name", true), key("id", false), key("price", false)]);
    assert_eq!(order_text(&keys), "name DESC, id, price");
    assert_eq!(parse_order("", &columns()), Ok(Vec::new()));
    assert!(parse_order("id desc desc", &columns()).is_err());
    assert_eq!(parse_order("x", &columns()), Err(ParseError::UnknownColumn("x".into())));
    assert_eq!(quote_name("Order"), "Order");
    assert_eq!(quote_name("order"), "order");
    assert_eq!(quote_name("not"), "\"not\"");
    assert_eq!(quote_name("first name"), "\"first name\"");
    assert_eq!(quote_name("1st"), "\"1st\"");
}

#[test]
fn a_header_click_cycles_ascending_descending_none() {
    let key = |c: &str, d| SortKey { column: c.into(), descending: d };
    let mut keys = Vec::new();
    cycle_sort(&mut keys, "a", false);
    assert_eq!(keys, vec![key("a", false)]);
    cycle_sort(&mut keys, "a", false);
    assert_eq!(keys, vec![key("a", true)]);
    cycle_sort(&mut keys, "a", false);
    assert!(keys.is_empty());
    // Another column alone: it replaces the order.
    cycle_sort(&mut keys, "a", false);
    cycle_sort(&mut keys, "b", false);
    assert_eq!(keys, vec![key("b", false)]);
}

#[test]
fn shift_clicks_add_columns_in_order() {
    let key = |c: &str, d| SortKey { column: c.into(), descending: d };
    let mut keys = vec![key("a", false)];
    cycle_sort(&mut keys, "b", true);
    assert_eq!(keys, vec![key("a", false), key("b", false)]);
    cycle_sort(&mut keys, "a", true);
    assert_eq!(keys, vec![key("a", true), key("b", false)]);
    cycle_sort(&mut keys, "a", true);
    assert_eq!(keys, vec![key("b", false)]);
    // A plain click on one of several: only it, ascending.
    cycle_sort(&mut keys, "c", true);
    cycle_sort(&mut keys, "b", false);
    assert_eq!(keys, vec![key("b", false)]);
}
