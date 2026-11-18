use kernel::Rgba;

use super::*;

fn set(tex: &str, display: bool) -> Arc<Formula> {
    let text = math_text();
    let mut text = text.lock().expect("the math font");
    match typeset(tex, display, 16.0, Rgba::WHITE, Rgba::rgb(0xff0000), &mut text, 1.0) {
        Ok(f) => f,
        Err(e) => panic!("{tex}: {e}"),
    }
}

fn glyphs(f: &Formula) -> Vec<(f32, f32, String)> {
    f.scene
        .prims
        .iter()
        .filter_map(|p| match p {
            scene::Prim::Text(t) => Some((t.at.x, t.at.y, t.text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn scripts_rise_and_fall() {
    let f = set("x^2_i", false);
    let g = glyphs(&f);
    let (x, two, i) = (&g[0], &g[1], &g[2]);
    assert!(two.1 < x.1 && i.1 > x.1, "{g:?}");
    assert!(two.0 > x.0 && i.0 > x.0);
    assert!(f.ascent > 0.0 && f.descent > 0.0);
}

#[test]
fn fractions_have_a_bar_and_stack() {
    let f = set("\\frac{a+b}{2}", true);
    let g = glyphs(&f);
    let two = g.iter().find(|t| t.2 == "2").expect("denominator");
    let plus = g.iter().find(|t| t.2 == "+").expect("numerator");
    assert!(two.1 > plus.1);
    assert!(!f.masks.is_empty(), "the bar");
}

#[test]
fn everything_common_is_set() {
    for tex in [
        "\\sqrt[3]{x^2+1}",
        "\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}",
        "\\int_0^\\infty e^{-x^2} dx = \\frac{\\sqrt{\\pi}}{2}",
        "\\left( \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right)",
        "\\begin{bmatrix} 1 & 0 \\\\ 0 & 1 \\end{bmatrix}",
        "f(x) = \\begin{cases} x & x \\ge 0 \\\\ -x & \\text{otherwise} \\end{cases}",
        "\\hat{x} \\vec{v} \\overline{AB} \\dot{y} \\tilde{z}",
        "\\mathbb{R}^n \\mathcal{L} \\mathbf{v} \\lim_{x \\to 0} \\frac{\\sin x}{x}",
        "\\binom{n}{k} \\color{red}{x} \\nosuch",
    ] {
        let f = set(tex, true);
        assert!(f.scene.width > 10.0, "{tex}");
    }
}

#[test]
fn unbalanced_formulas_fail() {
    let text = math_text();
    let mut text = text.lock().expect("the math font");
    assert!(typeset("\\frac{a", false, 16.0, Rgba::WHITE, Rgba::WHITE, &mut text, 1.0).is_err());
}

#[test]
fn the_font_is_ours_and_goes_with_its_last_user() {
    let a = math_text();
    let b = math_text();
    assert!(Arc::ptr_eq(&a, &b), "one, shared");
    assert_eq!(a.lock().expect("font").families(), vec!["STIX Two Math".to_owned()]);
    // Held only by its users: none left, it's freed (other tests may hold it meanwhile).
    let weak = Arc::downgrade(&a);
    drop((a, b));
    if let Some(still) = weak.upgrade() {
        assert!(Arc::strong_count(&still) >= 2, "only by someone else");
    }
}
