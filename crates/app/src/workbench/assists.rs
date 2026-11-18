//! The modules' help with the documents they know ([`code_intelligence::DocumentAssist`]: a
//! query console's completion from its source's schema): asked before the frameworks and
//! the language, for the languages each says. A list shown from one is made again when
//! its generation changes (what it asked for in the background came).

use code_intelligence::{
    AssistContext, AssistTarget, DocumentAssist, FrameworkCompletion, HoverInfo,
};

use super::*;

impl Workbench {
    /// Asks the assists of the language at `offset` in `source` (the active document), in
    /// order, until one answers.
    fn ask_assists<R>(
        &self,
        source: &str,
        offset: usize,
        ask: impl Fn(&dyn DocumentAssist, &AssistContext<'_>) -> Option<R>,
    ) -> Option<R> {
        if self.support.assists.is_empty() {
            return None;
        }
        let tab = self.tabs.get(self.active)?;
        let path = tab.doc.path()?;
        let language = self.language_at(offset)?;
        let language = language.as_str();
        let own = tab.syntax.language().clone();
        let at = AssistContext {
            project: self.project.as_ref().map(|_| self.data.id),
            root: self.project.as_ref().map(FileTree::root_path),
            path,
            language,
            text: source,
            offset,
            version: tab.doc.version(),
        };
        self.support
            .assists
            .iter()
            .filter(|a| a.asked_for(language, own.as_str()))
            .find_map(|a| ask(a.as_ref(), &at))
    }

    /// The characters after which the assists of the language at `offset` suggest.
    pub(super) fn assist_triggers(&self, offset: usize) -> String {
        let Some(language) = self.language_at(offset) else { return String::new() };
        let Some(tab) = self.tabs.get(self.active) else { return String::new() };
        let own = tab.syntax.language().clone();
        self.support
            .assists
            .iter()
            .filter(|a| a.asked_for(language.as_str(), own.as_str()))
            .map(|a| a.triggers())
            .collect()
    }

    pub(super) fn assist_completion(
        &self,
        source: &str,
        offset: usize,
    ) -> Option<FrameworkCompletion> {
        self.ask_assists(source, offset, |a, at| a.complete(at))
    }

    pub(super) fn assist_definition(&self, source: &str, offset: usize) -> Option<AssistTarget> {
        self.ask_assists(source, offset, |a, at| a.definition(at))
    }

    /// A name an assist found written as another language's code at `offset` (a PHP class
    /// in a Twig comment's `@var`), with that language's engine.
    pub(super) fn assist_code(
        &self,
        source: &str,
        offset: usize,
    ) -> Option<(Arc<dyn LanguageIntelligence>, code_intelligence::Target)> {
        let Some(AssistTarget::Code { language, target }) = self.assist_definition(source, offset)
        else {
            return None;
        };
        let intel = self.support.intelligence.iter().find(|i| i.language().as_str() == language)?;
        Some((Arc::clone(intel), target))
    }

    pub(super) fn assist_hover(&self, source: &str, offset: usize) -> Option<HoverInfo> {
        self.ask_assists(source, offset, |a, at| a.hover(at))
    }

    /// The signature of the function whose name ends at `offset` (its call's parameter
    /// help), as an assist knows it.
    pub(super) fn assist_signature(&self, source: &str, offset: usize) -> Option<String> {
        self.ask_assists(source, offset, |a, at| a.signature(at))
    }

    /// The hover card the assists give at `offset` of the active document (the snapshots').
    #[cfg(feature = "snapshot")]
    pub fn assist_hover_at(&self, offset: usize) -> Option<HoverInfo> {
        let source = self.tabs.get(self.active)?.text();
        self.assist_hover(&source, offset)
    }

    /// The assists' generation for the open project (their sum: any change shows).
    pub(super) fn assists_generation(&self) -> u64 {
        let project = self.project.as_ref().map(|_| self.data.id);
        self.support.assists.iter().fold(0u64, |sum, a| sum.wrapping_add(a.generation(project)))
    }

    /// A list shown from an assist whose data changed since (the columns it asked for
    /// came): it's due again (made at the next frame, [`Self::refresh_assisted_completion`]).
    pub fn poll_assists(&mut self) -> bool {
        self.completion.is_open()
            && self.completion_assist.is_some_and(|shown| shown != self.assists_generation())
    }

    /// The list of an assist, made again when its data changed.
    pub(super) fn refresh_assisted_completion(&mut self, text: &mut TextSystem) {
        if self.poll_assists() {
            self.update_completion(text);
        }
    }
}
