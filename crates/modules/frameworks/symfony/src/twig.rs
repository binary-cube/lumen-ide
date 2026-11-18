//! What Symfony adds to Twig: its tags, filters and functions, `app`, and the variables a
//! controller passes to the template (gathered ahead, in the background; the suggestions
//! only read what was gathered).

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use code_intelligence::template::{Passed, PassedFiles, TwigPlace, twig_place};
use code_intelligence::{
    CompletionKind, FrameworkCompletion, FrameworkContext, Vocabulary, suggest,
};

/// Where the words come from: replacing this file (by a generated one) is all it takes.
const DATA: &str = include_str!("../data/twig.data");

/// How a controller names a template.
const RENDERS: &[&str] = &["->render(", "->renderView(", "->renderForm(", "->renderBlock("];

/// Where the templates are, under the project's root.
const TEMPLATES: &str = "templates/";

#[derive(Debug, Default)]
pub(crate) struct Twig {
    words: OnceLock<Vocabulary>,
    files: Mutex<PassedFiles>,
    passed: Mutex<Arc<Passed>>,
}

impl Twig {
    /// Brings up to date what the controllers (`sources`, under `src`) pass.
    pub(crate) fn prepare(&self, at: &FrameworkContext<'_>, sources: &[String]) {
        let files: Vec<String> = sources.iter().map(|f| format!("src/{f}")).collect();
        let mut known = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        let passed = known.update(at.project, &files, RENDERS);
        *self.passed.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(passed);
    }

    /// The name this template is rendered by.
    fn template_name(at: &FrameworkContext<'_>) -> Option<String> {
        let relative = at.path.strip_prefix(at.project.root?).ok()?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        relative.strip_prefix(TEMPLATES).map(str::to_owned)
    }

    /// The suggestions at `at`, if it's inside a Twig delimiter of a template.
    pub(crate) fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if at.path.extension().is_none_or(|e| e != "twig") {
            return None;
        }
        let (place, replace) = twig_place(at.text, at.offset)?;
        if replace.is_empty() && !matches!(place, TwigPlace::Filter | TwigPlace::Member { .. }) {
            return None;
        }
        use CompletionKind::{Function, Keyword, Property, Variable};
        let groups: &[(&str, CompletionKind)] = match &place {
            TwigPlace::Tag => &[("tag", Keyword)],
            TwigPlace::Filter => &[("filter", Function)],
            TwigPlace::Test => &[("test", Keyword)],
            TwigPlace::Expression => &[("function", Function)],
            TwigPlace::Member { object } if object == "app" => &[("app", Property)],
            TwigPlace::Member { .. } => return None,
        };
        let query = &at.text[replace.clone()];
        let words = self.words.get_or_init(|| Vocabulary::parse(DATA));
        let mut items = words.suggest(groups, query);
        if place == TwigPlace::Expression {
            let passed = Arc::clone(&self.passed.lock().unwrap_or_else(PoisonError::into_inner));
            let given = Self::template_name(at).and_then(|name| passed.get(&name).cloned());
            let names = given.iter().flatten().map(String::as_str).chain(["app"]);
            items.extend(suggest(query, names, Variable, None));
            items.sort_by(|a, b| b.score.cmp(&a.score).then(a.label.cmp(&b.label)));
        }
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }
}

#[cfg(test)]
#[path = "../tests/unit/twig.rs"]
mod tests;
