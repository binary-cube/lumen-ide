use super::*;

#[test]
fn integers_are_summed_exactly() {
    let mut a = Aggregates::new();
    for v in [CellRef::Int(i64::MAX), CellRef::Int(1), CellRef::Null, CellRef::Text("x")] {
        a.add(&v);
    }
    assert_eq!(a.count, 4);
    assert_eq!(a.numbers, 2);
    assert_eq!(a.sum_text(), "9223372036854775808");
    assert_eq!(a.min_text(), "1");
}

#[test]
fn decimals_and_numbers_kept_as_text_count() {
    let mut a = Aggregates::new();
    for v in [CellRef::Float(1.5), CellRef::Other("2.25"), CellRef::Int(-1), CellRef::Other("n/a")]
    {
        a.add(&v);
    }
    assert_eq!(a.numbers, 3);
    assert_eq!(a.sum_text(), "2.75");
    assert_eq!(a.average().map(number), Some("0.9167".to_owned()));
    assert_eq!(a.min_text(), "-1");
    assert_eq!(a.max_text(), "2.25");
}

#[test]
fn without_numbers_only_the_count() {
    let mut a = Aggregates::new();
    a.add(&CellRef::Text("a"));
    assert_eq!((a.count, a.numbers, a.average()), (1, 0, None));
}

#[test]
fn figures_drop_trailing_zeros() {
    assert_eq!(number(3.0), "3");
    assert_eq!(number(0.1 + 0.2), "0.3");
    assert_eq!(number(-0.00001), "0");
    assert_eq!(number(1e20), "100000000000000000000");
}
