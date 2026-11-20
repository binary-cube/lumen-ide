use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use refactor::NameStyle;
use refactor::application::{
    Conflict, ConflictCheck, RenameTarget, Resolution, Resolver, Scope, Sources,
};

use super::*;

struct Files(HashMap<PathBuf, String>);

impl Sources for Files {
    fn candidates(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.0.keys().cloned().collect();
        paths.sort();
        paths
    }
    fn read(&self, path: &Path) -> Option<String> {
        self.0.get(path).cloned()
    }
}

/// After `->`: the symbol; anything else: unknown.
struct Arrow;

impl Resolver for Arrow {
    fn resolve(&self, _: &Path, text: &str, range: Range<usize>) -> Resolution {
        if text[..range.start].ends_with("->") || text[..range.start].ends_with("function ") {
            Resolution::Target
        } else {
            Resolution::Unknown
        }
    }
}

struct None_;

impl ConflictCheck for None_ {
    fn conflicts(&self, _: &str) -> Vec<Conflict> {
        Vec::new()
    }
}

/// A view showing the usages of `send` in `text`.
fn view_of(text: &str) -> UsagesView {
    let sources = Files(HashMap::from([(PathBuf::from("/p/a.php"), text.to_owned())]));
    let target = RenameTarget {
        name: "send".into(),
        style: NameStyle::CODE.with_sigil(Some('$')),
        scope: Scope::Project,
        file: None,
    };
    let context = RenameContext {
        target: target.clone(),
        sources: Arc::new(sources),
        resolver: Arc::new(Arrow),
        conflicts: Arc::new(None_),
    };
    let mut view = UsagesView::default();
    view.set_data(&UsagesData { context: context.clone() });
    let found =
        plan_rename(&target, context.sources.as_ref(), context.resolver.as_ref(), &|| false);
    view.set_usages(found);
    view
}

#[test]
fn lists_every_place_read_only_and_opens_them() {
    let mut view = view_of("<?php function send() {}\n$m->send();\nsend();");
    let usages = view.usages().unwrap();
    let places: Vec<(Certainty, u32)> =
        usages.files[0].occurrences.iter().map(|o| (o.certainty, o.line)).collect();
    assert_eq!(
        places,
        [(Certainty::Certain, 0), (Certainty::Certain, 1), (Certainty::Uncertain, 2)]
    );
    assert!(view.summary().contains('3') || view.summary().contains("3"));
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
}

/// A group too short for the header and a few places: the page's thumb, pressed and
/// dragged, scrolls it.
#[test]
fn the_page_scrolls_by_its_thumb() {
    let mut view = view_of("<?php function send() {}\nsend();\nsend();\nsend();");
    let (w, h) = (600.0, 260.0);
    crate::page::tests::paint(&mut view, w, h);
    crate::page::tests::drag_thumb(&mut view, w, 30.0);
    assert!(view.page.offset() > 0.0, "the page scrolled");
}
