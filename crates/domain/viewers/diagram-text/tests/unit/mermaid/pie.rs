use super::super::{Diagram, parse as parse_any};

#[test]
fn slices_title_and_data() {
    let p =
        parse_any("pie showData title Pets\n  \"Dogs\" : 386\n  \"Cats\" : 85.5\n  \"Rats\" : 15")
            .expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("Pets"));
    let Diagram::Pie(pie) = p.diagram else { panic!() };
    assert!(pie.show_data);
    assert_eq!(
        pie.slices,
        vec![("Dogs".into(), 386.0), ("Cats".into(), 85.5), ("Rats".into(), 15.0)]
    );
    assert_eq!(parse_any("pie\n\"A\" : x").unwrap_err().line, 2);
}
