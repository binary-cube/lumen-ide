use std::ops::Range;
use std::path::{Path, PathBuf};

use refactor::application::{Resolution, Resolver, Sources};
use refactor::signature::{CallKind, CallRules, SignatureSyntax, SignatureTarget, split_list};

use super::*;

struct One(String);

impl Sources for One {
    fn candidates(&self) -> Vec<PathBuf> {
        vec!["/p/a.php".into()]
    }
    fn read(&self, _: &Path) -> Option<String> {
        Some(self.0.clone())
    }
}

struct All;

impl Resolver for All {
    fn resolve(&self, _: &Path, _: &str, _: Range<usize>) -> Resolution {
        Resolution::Target
    }
}

/// `$name = default` parameters.
#[derive(Debug)]
struct Simple;

impl SignatureSyntax for Simple {
    fn calls(&self) -> CallRules {
        CallRules {
            member_access: &["->", "::"],
            declares: &["function"],
            constructs: Some("new"),
            sigil: Some('$'),
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["php"]
    }
    fn parse(&self, text: &str) -> Param {
        let (name, default) = text.split_once('=').unwrap_or((text, ""));
        Param {
            name: name.trim().trim_start_matches('$').to_owned(),
            default: default.trim().to_owned(),
            ..Param::default()
        }
    }
    fn render(&self, p: &Param) -> String {
        if p.default.is_empty() {
            format!("${}", p.name)
        } else {
            format!("${} = {}", p.name, p.default)
        }
    }
}

fn view(text: &str) -> SignatureView {
    let open = text.find('(').unwrap();
    let (params, items) = split_list(text, open).unwrap();
    let old = items.iter().map(|r| Simple.parse(&text[r.clone()])).collect();
    let target = SignatureTarget {
        name: "send".into(),
        kind: CallKind::Function,
        path: "/p/a.php".into(),
        params,
        old,
    };
    let context = SignatureContext {
        target: target.clone(),
        sources: Arc::new(One(text.to_owned())),
        resolver: Arc::new(All),
        syntax: Arc::new(Simple),
    };
    let mut view = SignatureView::default();
    view.set_data(&SignatureData { context });
    view.set_calls(find_calls(&target, &Simple, &One(text.to_owned()), &All, &|| false));
    view
}

#[test]
fn reordering_and_adding_rewrites_the_calls() {
    let text = "<?php function send($to, $body) {}\nsend('a', 'b');";
    let mut view = view(text);
    assert!(!view.can_apply(), "nothing changed yet");
    view.move_row(1, -1);
    view.add_row();
    // The new parameter (focused): its name, then the value the calls pass.
    view.edit_key(ViewerKey::Backspace);
    view.type_text("urgent");
    // Tab: the type, the default, then the value.
    for _ in 0..3 {
        assert!(view.edit_key(ViewerKey::Tab), "Tab moves to the next field");
    }
    view.type_text("true");
    assert!(view.can_apply());
    let plan = view.plan().unwrap();
    let edits = plan.edits();
    let out = refactor::apply_edits(text, &edits[0].edits).unwrap();
    assert_eq!(out, "<?php function send($body, $to, $urgent) {}\nsend('b', 'a', true);");
}

#[test]
fn removing_drops_the_argument_and_bad_names_block_applying() {
    let text = "<?php function send($to, $body) {}\nsend('a', 'b');";
    let mut view = view(text);
    view.remove_row(0);
    let out = refactor::apply_edits(text, &view.plan().unwrap().edits()[0].edits).unwrap();
    assert_eq!(out, "<?php function send($body) {}\nsend('b');");
    view.focus_field(0, NAME);
    view.edit_key(ViewerKey::End);
    view.type_text("-x");
    assert!(!view.valid() && !view.can_apply());
}

/// A group too short for the form and a few places: the page's thumb, pressed and dragged,
/// scrolls it.
#[test]
fn the_page_scrolls_by_its_thumb() {
    let mut view = view("<?php function send($to, $body) {}\nsend('a', 'b');");
    let (w, h) = (900.0, 260.0);
    crate::page::tests::paint(&mut view, w, h);
    crate::page::tests::drag_thumb(&mut view, w, 30.0);
    assert!(view.page.offset() > 0.0, "the page scrolled");
}
