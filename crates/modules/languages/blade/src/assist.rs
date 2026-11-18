//! Blade's own suggestions: its directives after `@`, the variables the template declares,
//! `$loop`'s members inside a loop. Read from the text around the cursor; what only the
//! project knows is left to the framework.

use std::sync::OnceLock;

use code_intelligence::template::{BladePlace, blade_place};
use code_intelligence::{
    AssistContext, CompletionKind, DocumentAssist, FrameworkCompletion, Vocabulary, suggest,
};

/// Where the words come from: replacing this file (by a generated one) is all it takes.
const DATA: &str = include_str!("../data/blade.data");

/// Where `@` and `$` are that language's own (`@media`, a script's `$el`).
const FOREIGN: &[&str] = &["css", "javascript", "typescript"];

#[derive(Debug, Default)]
pub struct BladeAssist {
    words: OnceLock<Vocabulary>,
}

impl BladeAssist {
    fn words(&self) -> &Vocabulary {
        self.words.get_or_init(|| Vocabulary::parse(DATA))
    }
}

impl DocumentAssist for BladeAssist {
    fn id(&self) -> &'static str {
        "lang.blade"
    }

    fn languages(&self) -> &'static [&'static str] {
        &["blade"]
    }

    fn triggers(&self) -> &'static str {
        "@$>"
    }

    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        if !at.path.to_string_lossy().ends_with(".blade.php") || FOREIGN.contains(&at.language) {
            return None;
        }
        let (place, replace) = blade_place(at.text, at.offset)?;
        let query = &at.text[replace.clone()];
        let items = match &place {
            BladePlace::Directive => {
                self.words().suggest(&[("directive", CompletionKind::Keyword)], query)
            }
            BladePlace::Variable => {
                let visible = crate::variables::visible(at.text, replace.start);
                suggest(
                    query,
                    visible.names.iter().map(String::as_str),
                    CompletionKind::Variable,
                    None,
                )
            }
            BladePlace::Member { object } if object == "loop" => {
                if !crate::variables::visible(at.text, replace.start).in_loop {
                    return None;
                }
                self.words().suggest(&[("loop", CompletionKind::Property)], query)
            }
            BladePlace::Member { .. } => return None,
        };
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }
}

#[cfg(test)]
#[path = "../tests/unit/assist.rs"]
mod tests;
