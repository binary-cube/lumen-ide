//! What the modules hide in the documents of their languages
//! ([`workbench_model::Concealment`]: a `.env` file's secrets): asked again only when the
//! document's version or a contribution's generation changes; a hidden part is shown while
//! the pointer is over it or a caret is on one of its lines.

use std::ops::Range;

use workbench_model::concealment;

use super::*;

/// A tab's hidden parts.
#[derive(Debug, Default)]
pub(super) struct TabConcealed {
    /// What they were asked for: the document's version, the contributions' generations.
    key: Option<(u64, u64)>,
    /// What the modules hide (sorted).
    all: Vec<Range<usize>>,
    /// What's drawn hidden now: `all` but what the pointer or a caret shows.
    pub(super) hidden: Vec<Range<usize>>,
}

impl TabConcealed {
    /// The hidden part `offset` is in (its place in the list).
    fn under(&self, offset: usize) -> Option<usize> {
        self.all.iter().position(|r| r.start <= offset && offset <= r.end)
    }
}

impl Tab {
    /// What's drawn hidden, brought up to date: what's under `pointer` (a byte of the text)
    /// or on a caret's line shown.
    pub(super) fn sync_concealed(&mut self, support: &LanguageSupport, pointer: Option<usize>) {
        if support.concealments.is_empty() {
            return;
        }
        let language = self.syntax.language().as_str();
        let providers: Vec<&Arc<dyn workbench_model::Concealment>> =
            support.concealments.iter().filter(|c| c.languages().contains(&language)).collect();
        let generation = providers.iter().fold(0u64, |g, c| g.wrapping_add(c.generation()));
        let key = Some((self.doc.version(), generation));
        if self.concealed.key != key {
            self.concealed.key = key;
            let text = self.text();
            let mut all: Vec<Range<usize>> =
                providers.iter().flat_map(|c| c.concealed(&text)).collect();
            all.sort_by_key(|r| r.start);
            self.concealed.all = all;
        }
        if self.concealed.all.is_empty() {
            self.concealed.hidden.clear();
            return;
        }
        let buffer = self.doc.buffer();
        let carets: Vec<usize> =
            self.doc.selections().iter().map(|s| buffer.line_of(s.head)).collect();
        let shown = |r: &Range<usize>| {
            let lines = buffer.line_of(r.start)..=buffer.line_of(r.end);
            pointer.is_some_and(|p| r.start <= p && p <= r.end)
                || carets.iter().any(|l| lines.contains(l))
        };
        self.concealed.hidden = concealment::hidden(&self.concealed.all, shown);
    }
}

impl Workbench {
    /// The pointer moved to byte `offset` of the active document's text (`None`: off it):
    /// `true` if that shows or hides a hidden part (a redraw).
    pub(super) fn set_pointer_text(&mut self, offset: Option<usize>) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let doc = tab.doc.id();
        let under = |at: Option<(DocumentId, usize)>| {
            at.filter(|(d, _)| *d == doc).and_then(|(_, o)| tab.concealed.under(o))
        };
        let before = under(self.pointer_text);
        self.pointer_text = offset.map(|o| (doc, o));
        before != under(self.pointer_text)
    }

    /// The active document's byte under the pointer, if it's on its text.
    pub(super) fn pointer_offset(&self) -> Option<usize> {
        let doc = self.tabs.get(self.active)?.doc.id();
        self.pointer_text.filter(|(d, _)| *d == doc).map(|(_, o)| o)
    }
}
