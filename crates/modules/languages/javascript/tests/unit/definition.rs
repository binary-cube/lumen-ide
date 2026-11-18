use super::*;
use syntax_ts::tree_sitter::Parser;

fn find(src: &str, at: &str, nth: usize) -> Option<usize> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).ok()?;
    let tree = parser.parse(src, None)?;
    let offset = src.match_indices(at).nth(nth)?.0;
    local_definition(tree.root_node(), src, offset)
}

const SRC: &str = r##"
import { render as draw, h } from './dom.js';
(function () {
  var BG_LIGHT = "#fff";
  function create(doc, { theme, size: big = 2 }, ...rest) {
    let canvas = null;
    if (doc) { var late = 1; }
    function apply() {
      const bg = theme === "dark" ? BG_DARK : BG_LIGHT;
      canvas.textContent = bg + big + late + rest.length;
      draw(h);
    }
    try { apply(); } catch (err) { console.log(err); }
    for (const item of doc.items) { use(item); }
  }
})();
"##;

#[test]
fn resolves_by_scope() {
    let decl = |name: &str, nth: usize| SRC.match_indices(name).nth(nth).map(|(i, _)| i);
    // A `var` in the enclosing IIFE.
    assert_eq!(find(SRC, "BG_LIGHT;", 0), decl("BG_LIGHT", 0));
    // A local of the outer function, used in the nested one.
    assert_eq!(find(SRC, "canvas.textContent", 0), decl("canvas", 0));
    // A destructured parameter, an aliased one, and a rest parameter.
    assert_eq!(find(SRC, "theme ===", 0), decl("theme", 0));
    assert_eq!(find(SRC, "big +", 0), decl("big", 0));
    assert_eq!(find(SRC, "rest.length", 0), decl("rest", 0));
    // A `var` hoisted out of a block.
    assert_eq!(find(SRC, "late +", 0), decl("late", 0));
    // A nested function, a catch parameter, a for-of variable, an import alias.
    assert_eq!(find(SRC, "apply();", 0), decl("apply", 0));
    assert_eq!(find(SRC, "err)", 1), decl("err", 0));
    assert_eq!(find(SRC, "item)", 0), decl("item", 0));
    assert_eq!(find(SRC, "draw(h)", 0), decl("draw", 0));
    // Not declared in the file / a property: left to the project index.
    assert_eq!(find(SRC, "BG_DARK", 0), None);
    assert_eq!(find(SRC, "textContent", 0), None);
}
