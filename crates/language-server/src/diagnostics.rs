//! What's wrong in an open document, as LSP diagnostics: its syntax, its unused and
//! deprecated names and, once the project is indexed, what depends on the rest of it.

use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::sync::PoisonError;

use code_intelligence::Types;
use document::Document;
use lsp_protocol::text::range_value;
use serde_json::{Value, json};
use syntax_ts::{SyntaxErrorKind, parse_document};

use crate::session::Session;

const ERROR: u8 = 1;
const WARNING: u8 = 2;
const HINT: u8 = 4;
const TAG_UNNECESSARY: u8 = 1;
const TAG_DEPRECATED: u8 = 2;

impl<W: Write> Session<W> {
    /// `path`'s diagnostics (`project`: its checks needing the whole index too).
    pub fn diagnostics(&self, path: &Path, project: bool) -> Vec<Value> {
        let (Some(open), Some(engine)) = (self.open.get(path), self.engines.of(path)) else {
            return Vec::new();
        };
        // A template read with HTML's engine would be full of false errors.
        if self.engines.is_hosted(path) {
            return Vec::new();
        }
        let text = open.lines.text();
        let lines = &open.lines;
        let one = |range: &Range<usize>, severity: u8, message: String, tag: Option<u8>| {
            let (start, end) = lines.range(range, self.encoding);
            let mut value = json!({
                "range": range_value(start, end),
                "severity": severity,
                "source": "language-server",
                "message": message,
            });
            if let Some(tag) = tag {
                value["tags"] = json!([tag]);
            }
            value
        };
        let snapshot = Document::new(text).buffer().snapshot();
        let parsed =
            parse_document(&self.engines.syntax, &engine.language(), &snapshot, None, &[], 0);
        let mut out: Vec<Value> = parsed
            .iter()
            .flat_map(|tree| tree.errors())
            .map(|e| {
                let message = match &e.kind {
                    SyntaxErrorKind::Missing(token) => {
                        format!("{}: {token}", i18n::t("error.missing"))
                    }
                    SyntaxErrorKind::Unexpected => i18n::t("error.invalid"),
                };
                one(&e.range, ERROR, message, None)
            })
            .collect();
        for unused in engine.unused(text) {
            let message =
                format!("{}: {}", i18n::t(&format!("unused.{}", unused.kind)), unused.name);
            out.push(one(&unused.range, HINT, message, Some(TAG_UNNECESSARY)));
        }
        let mut deprecated = engine.deprecated(text);
        if project {
            let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
            let checked = engine.check(text, &Types { index: &index, path });
            deprecated.extend(checked.deprecated);
            for problem in checked.problems {
                let key = format!("problem.{}", problem.kind);
                let message = match problem.arguments {
                    Some((expected, given)) => {
                        i18n::tr!(&key, expected = expected as i64, given = given as i64)
                    }
                    None => i18n::t(&key),
                };
                out.push(one(
                    &problem.range,
                    WARNING,
                    format!("{message}: {}", problem.name),
                    None,
                ));
            }
        }
        for d in deprecated {
            let message = format!("{}: {}", i18n::t(&format!("deprecated.{}", d.kind)), d.name);
            out.push(one(&d.range, HINT, message, Some(TAG_DEPRECATED)));
        }
        out
    }
}
