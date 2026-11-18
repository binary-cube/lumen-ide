//! Twig's own suggestions: read from the text around the cursor, so a tag still being typed
//! (not parsed as one yet, the HTML around it at the cursor) counts too. A string is left to
//! whoever knows what it names (a framework's routes and templates).

use std::sync::OnceLock;

use code_intelligence::template::{TwigPlace as Place, twig_place};
use code_intelligence::{
    AssistContext, CompletionKind, DocumentAssist, FrameworkCompletion, Vocabulary,
};

/// Where the words come from: replacing this file (by a generated one) is all it takes.
const DATA: &str = include_str!("../data/twig.data");

#[derive(Debug, Default)]
pub struct TwigAssist {
    words: OnceLock<Vocabulary>,
}

impl TwigAssist {
    fn words(&self) -> &Vocabulary {
        self.words.get_or_init(|| Vocabulary::parse(DATA))
    }
}

impl DocumentAssist for TwigAssist {
    fn id(&self) -> &'static str {
        "lang.twig"
    }

    fn languages(&self) -> &'static [&'static str] {
        &["twig"]
    }

    fn triggers(&self) -> &'static str {
        "|"
    }

    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        if at.path.extension().is_none_or(|e| e != "twig") {
            return None;
        }
        let (place, replace) = twig_place(at.text, at.offset)?;
        // Right after the delimiter nothing's asked for yet; after `|` or `.` a list is.
        if replace.is_empty() && !matches!(place, Place::Filter | Place::Member { .. }) {
            return None;
        }
        use CompletionKind::{Function, Keyword, Property};
        let visible = crate::variables::visible(at.text, replace.start);
        let groups: &[(&str, CompletionKind)] = match &place {
            Place::Tag => &[("tag", Keyword)],
            Place::Filter => &[("filter", Function)],
            Place::Test => &[("test", Keyword)],
            Place::Expression => &[("function", Function), ("operator", Keyword)],
            Place::Member { object } if object == "loop" && visible.in_loop => {
                &[("loop", Property)]
            }
            Place::Member { .. } => return None,
        };
        let query = &at.text[replace.clone()];
        let mut items = self.words().suggest(groups, query);
        if place == Place::Expression {
            let own = code_intelligence::suggest(
                query,
                visible.names.iter().map(String::as_str),
                CompletionKind::Variable,
                None,
            );
            items = code_intelligence::servers::merge_completions(own, items);
        }
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }
}

#[cfg(test)]
#[path = "../tests/unit/assist.rs"]
mod tests;
