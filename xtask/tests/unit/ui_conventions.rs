use super::*;

/// A component without a page is told; one listed as a helper, or with its page, isn't.
#[test]
fn a_component_needs_its_page() {
    let lib = "pub mod data_grid;\npub mod painter;\npub mod charts;\nmod private;\n";
    let found = missing_pages(lib, &|page| page == "data-grid.md");
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("`charts`"));
}

/// A fixed color in drawing code is told; in a `const`, a comment or after the tests, not.
#[test]
fn a_color_is_named() {
    let text = "\
/// The sky.
const SKY: Rgba = Rgba::rgb(0x020a2e);
const PALETTE: [Rgba; 2] = [
    Rgba::rgb(0x111111),
    Rgba::rgb(0x222222),
];
fn paint() {
    // Rgba::rgb(0xffffff) in a comment
    let c = Rgba::rgb(0x1a1a1a);
}
#[cfg(test)]
fn t() { Rgba::rgb(0x333333); }
";
    let found = loose_colors("a.rs", text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("a.rs:9:"));
}

/// A module's own control is told; another drawing isn't.
#[test]
fn a_module_draws_no_control_of_its_own() {
    let text = "fn paint_scrollbar(&self) {}\nfn paint_rows(&self) {}\nfn draw_button() {}\n";
    let found = hand_drawn("m.rs", text);
    assert_eq!(found.len(), 2);
}
