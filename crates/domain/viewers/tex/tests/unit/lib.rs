use super::*;

fn sym(text: &str) -> Node {
    match symbol(text) {
        Some(n) => n,
        None => panic!("{text}"),
    }
}

#[test]
fn symbols_scripts_and_classes() {
    let n = parse("x^2 + y_{i,j} = \\alpha").expect("parsed");
    assert_eq!(n.len(), 5);
    let Node::Scripts { sup: Some(sup), sub: None, .. } = &n[0] else { panic!("{:?}", n[0]) };
    assert!(matches!(**sup, Node::Symbol { ref text, .. } if text == "2"));
    assert!(matches!(&n[1], Node::Symbol { class: Class::Bin, .. }));
    assert!(matches!(&n[2], Node::Scripts { sub: Some(_), sup: None, .. }));
    assert!(matches!(&n[3], Node::Symbol { class: Class::Rel, .. }));
    assert_eq!(n[4], sym("alpha"));
    // Both scripts on one base, in either order; a prime is a superscript.
    let n = parse("a_1^2 b^2_1 f'").expect("parsed");
    assert!(
        n.iter().take(2).all(|n| matches!(n, Node::Scripts { sub: Some(_), sup: Some(_), .. }))
    );
    assert!(matches!(&n[2], Node::Scripts { sup: Some(_), .. }));
}

#[test]
fn fractions_roots_operators_and_delimiters() {
    let n = parse("\\frac{a}{b} \\binom n k \\sqrt[3]{x} \\sum_{i=0}^{n} \\lim_{x \\to 0} \\left( \\frac12 \\right]").expect("parsed");
    assert!(matches!(&n[0], Node::Frac { bar: true, .. }));
    assert!(matches!(&n[1], Node::Frac { bar: false, parens: true, .. }));
    assert!(matches!(&n[2], Node::Sqrt { index: Some(_), .. }));
    let Node::Scripts { base, sub: Some(_), sup: Some(_) } = &n[3] else { panic!("{:?}", n[3]) };
    assert!(matches!(**base, Node::BigOp { limits: true, symbol: true, .. }));
    assert!(matches!(&n[4], Node::Scripts { .. }));
    let Node::Delimited { left, right, body } = &n[5] else { panic!() };
    assert_eq!((left.as_str(), right.as_str()), ("(", "]"));
    assert!(matches!(&body[0], Node::Frac { .. }));
}

#[test]
fn environments_text_fonts_accents() {
    let n = parse("\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}").expect("parsed");
    let Node::Matrix { env, rows, .. } = &n[0] else { panic!() };
    assert_eq!((*env, rows.len(), rows[0].len()), (Env::PMatrix, 2, 2));
    let n = parse("f(x) = \\begin{cases} 1 & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}")
        .expect("parsed");
    assert!(n.iter().any(|n| matches!(n, Node::Matrix { env: Env::Cases, .. })));
    let n = parse("\\mathbb{R} \\hat{x} \\text{a b} \\operatorname{rank} \\quad \\color{red}{x}")
        .expect("parsed");
    assert!(matches!(&n[0], Node::Symbol { font: Font::DoubleStruck, .. }));
    assert!(matches!(&n[1], Node::Accent { accent: Accent::Hat, .. }));
    assert_eq!(n[2], Node::Text { text: "a b".into(), font: Font::Roman });
    assert!(matches!(&n[3], Node::BigOp { symbol: false, .. }));
    assert_eq!(n[4], Node::Space(1.0));
    assert!(matches!(&n[5], Node::Colored { .. }));
}

#[test]
fn errors() {
    assert_eq!(parse("\\frac{a}{b").unwrap_err().kind, ParseErrorKind::Unclosed);
    assert_eq!(parse("a}").unwrap_err().kind, ParseErrorKind::Unopened);
    assert!(matches!(
        parse("\\begin{pmatrix} 1").unwrap_err().kind,
        ParseErrorKind::Environment(_)
    ));
    assert_eq!(parse("\\left( x").unwrap_err().kind, ParseErrorKind::Right);
    // An unknown command is kept, shown as an error.
    assert_eq!(parse("\\nosuch x").expect("parsed")[0], Node::Error("\\nosuch".into()));
}
