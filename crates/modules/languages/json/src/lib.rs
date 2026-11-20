//! JSON module: language, grammar, content detector, folding with the number of keys.
//!
//! The content detector makes JSON inside strings (PHP, JS, HTML attributes) be
//! recognized and shown as JSON, with colors and folding.

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{Confidence, ContentSniffer, LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("json");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.json"),
    name: "JSON",
    description: "JSON: colors, folding on objects and arrays, detection in other languages",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-json", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct JsonModule;

impl Module for JsonModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(
            LanguageDescriptor::new("json", "JSON")
                .icon_sample(3, "package.json")
                .extensions(&["json", "jsonc", "webmanifest", "har"])
                .file_names(&["composer.lock", ".babelrc", ".prettierrc", ".eslintrc", ".jshintrc"])
                .line_comment("//")
                .block_comment("/*", "*/")
                .icon("file-json"),
        );
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn ContentSniffer>>(Arc::new(JsonSniffer));
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.json",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_json::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: "".into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        embedded_errors: false,
        injected_errors: true,
        own_errors: true,
    }
}

/// Recognizes JSON: a structurally valid object or array (brackets, strings, values).
#[derive(Debug, Default)]
pub struct JsonSniffer;

impl ContentSniffer for JsonSniffer {
    fn language(&self) -> LanguageId {
        LANGUAGE
    }

    fn may_start_with(&self, first: char) -> bool {
        matches!(first, '{' | '[')
    }

    fn sniff(&self, text: &str) -> Option<Confidence> {
        let t = text.trim();
        let (first, last) = (t.chars().next()?, t.chars().last()?);
        if !matches!((first, last), ('{', '}') | ('[', ']')) || !looks_like_json(t) {
            return None;
        }
        // `[]` or `{}` alone say little; a key-value pair does.
        Some(Confidence(if t.contains(':') || t.contains(',') { 90 } else { 50 }))
    }
}

/// Cheap structural validation, without allocations: balanced brackets, closed strings,
/// only JSON values between them.
fn looks_like_json(text: &str) -> bool {
    let mut depth = Vec::with_capacity(16);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '[' => depth.push(c),
            '}' => {
                if depth.pop() != Some('{') {
                    return false;
                }
            }
            ']' => {
                if depth.pop() != Some('[') {
                    return false;
                }
            }
            '"' => loop {
                match chars.next() {
                    Some('\\') => {
                        chars.next();
                    }
                    Some('"') => break,
                    Some(_) => {}
                    None => return false,
                }
            },
            ':' | ',' | '-' | '+' | '.' | 'e' | 'E' => {}
            c if c.is_ascii_digit() || c.is_whitespace() => {}
            't' | 'f' | 'n' => {
                // true / false / null
                let rest: String = chars.clone().take(4).collect();
                let word = match c {
                    't' => "rue",
                    'f' => "alse",
                    _ => "ull",
                };
                if !rest.starts_with(word) {
                    return false;
                }
                for _ in 0..word.len() {
                    chars.next();
                }
            }
            _ => return false,
        }
        if depth.is_empty() {
            // After the root closes, nothing but whitespace may follow.
            return chars.all(char::is_whitespace);
        }
    }
    false
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
