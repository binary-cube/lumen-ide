//! The languages served: their modules, activated as the app activates them. A language
//! more is one module more in [`modules`].

use std::path::Path;
use std::sync::Arc;

use code_intelligence::{DocumentAssist, Framework, LanguageIntelligence};
use extensions::{Module, ModuleRegistry, ModuleSelection};
use languages::{LanguageDescriptor, LanguageRegistry};
use syntax_ts::{Grammar, Grammars, InjectionProvider, SyntaxConfig};

/// The languages written over HTML, without an engine of their own.
const HOSTED: &[&str] = &["twig", "blade"];

fn modules() -> Vec<Box<dyn Module>> {
    vec![
        Box::new(lang_php::PhpModule),
        Box::new(lang_html::HtmlModule),
        Box::new(lang_css::CssModule),
        Box::new(lang_javascript::JavaScriptModule),
        Box::new(lang_sql::SqlModule),
        Box::new(lang_twig::TwigModule),
        Box::new(lang_blade::BladeModule),
        Box::new(fw_laravel::LaravelModule),
        Box::new(fw_symfony::SymfonyModule),
        Box::new(fw_yii::YiiModule),
        Box::new(fw_vue::VueModule),
        Box::new(fw_react::ReactModule),
        Box::new(fw_htmx::HtmxModule),
    ]
}

/// What the modules contributed that answering needs.
pub struct Engines {
    pub intelligence: Vec<Arc<dyn LanguageIntelligence>>,
    pub frameworks: Vec<Arc<dyn Framework>>,
    languages: LanguageRegistry,
    pub assists: Vec<Arc<dyn DocumentAssist>>,
    pub syntax: SyntaxConfig,
}

impl Engines {
    pub fn load() -> Self {
        let mut registry = ModuleRegistry::default();
        for module in modules() {
            registry.register(module);
        }
        let (mut contributions, _) = registry.activate(&ModuleSelection::default());
        let mut languages = LanguageRegistry::default();
        for language in contributions.take::<LanguageDescriptor>() {
            languages.register(language);
        }
        let mut grammars = Grammars::default();
        for grammar in contributions.take::<Grammar>() {
            grammars.register(grammar);
        }
        let mut syntax = SyntaxConfig::new(grammars);
        for provider in contributions.take::<Arc<dyn InjectionProvider>>() {
            syntax.add_provider(provider);
        }
        for dirs in contributions.take::<kernel::ProjectDirs>() {
            kernel::paths::register(dirs);
        }
        Self {
            intelligence: contributions.take(),
            frameworks: contributions.take(),
            languages,
            assists: contributions.take(),
            syntax,
        }
    }

    /// `path`'s language.
    pub fn language_of(&self, path: &Path) -> String {
        self.languages.detect(path, "").to_string()
    }

    fn engine_named(&self, language: &str) -> Option<&Arc<dyn LanguageIntelligence>> {
        self.intelligence.iter().find(|i| i.language().as_str() == language)
    }

    /// `path`'s language has no engine of its own: it's a template over HTML's.
    pub fn is_hosted(&self, path: &Path) -> bool {
        HOSTED.contains(&self.language_of(path).as_str())
    }

    /// The engine of `path`'s language; a template's is what it's written over, a language
    /// without a description goes by the extensions the engines say.
    pub fn of(&self, path: &Path) -> Option<&Arc<dyn LanguageIntelligence>> {
        let language = self.language_of(path);
        if HOSTED.contains(&language.as_str()) {
            return self.engine_named("html");
        }
        self.engine_named(&language).or_else(|| {
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            self.intelligence.iter().find(|i| i.extensions().contains(&extension.as_str()))
        })
    }

    /// The characters after which a client should ask for suggestions.
    pub fn triggers(&self) -> Vec<String> {
        let mut all: Vec<char> =
            self.intelligence.iter().flat_map(|i| i.completion_triggers().chars()).collect();
        all.extend(self.assists.iter().flat_map(|a| a.triggers().chars()));
        all.sort_unstable();
        all.dedup();
        all.into_iter().map(String::from).collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/engines.rs"]
mod tests;
