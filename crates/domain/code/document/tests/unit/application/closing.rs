use std::collections::HashSet;

use super::*;

/// Documents in memory: which are open, which modified.
#[derive(Default)]
struct Docs {
    open: HashSet<DocumentId>,
    modified: HashSet<DocumentId>,
}

impl OpenDocuments for Docs {
    fn is_open(&self, id: DocumentId) -> bool {
        self.open.contains(&id)
    }
    fn is_modified(&self, id: DocumentId) -> bool {
        self.modified.contains(&id)
    }
}

fn ids(n: usize) -> Vec<DocumentId> {
    (0..n).map(|_| DocumentId::next()).collect()
}

/// Unmodified ones close one after another, in order; then what follows.
#[test]
fn unmodified_documents_close_in_order() {
    let [a, b] = ids(2)[..] else { unreachable!() };
    let docs = Docs { open: [a, b].into(), ..Docs::default() };
    let mut closing = CloseDocuments::new([a, b], "quit");
    assert_eq!(closing.next(&docs), CloseStep::Close(a));
    assert_eq!(closing.next(&docs), CloseStep::Close(b));
    assert_eq!(closing.next(&docs), CloseStep::Done);
    assert_eq!(closing.into_then(), "quit");
}

/// A modified one is asked about, and stays asked until resolved; a closed one is skipped.
#[test]
fn a_modified_document_is_asked_about_until_resolved() {
    let [a, b, c] = ids(3)[..] else { unreachable!() };
    let mut docs = Docs { open: [a, b].into(), modified: [a].into() };
    let mut closing = CloseDocuments::new([a, c, b], ());
    assert_eq!(closing.next(&docs), CloseStep::Ask(a));
    assert_eq!(closing.next(&docs), CloseStep::Ask(a), "still waiting");
    // Saved (or discarded) and closed by the caller.
    docs.open.remove(&a);
    closing.resolved();
    assert_eq!(closing.next(&docs), CloseStep::Close(b), "`c` was closed already: skipped");
    assert_eq!(closing.next(&docs), CloseStep::Done);
}
