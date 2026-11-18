//! htmx's event handlers as code: the values of `hx-on:click="…"`, `hx-on::after-request="…"`
//! (and the older `hx-on-click` / `hx-on="click: …"`) are JavaScript, wherever HTML is written.

use std::sync::OnceLock;

use languages::LanguageId;
use syntax_ts::tree_sitter::Query;
use syntax_ts::{Injection, InjectionContext, InjectionProvider, InjectionSource};

const HTML: LanguageId = LanguageId::new_static("html");

/// An `hx-on…` attribute's value.
const PATTERN: &str = r#"
(attribute
  (attribute_name) @name
  (quoted_attribute_value (attribute_value) @value)
  (#match? @name "^hx-on"))
"#;

#[derive(Debug, Default)]
pub struct EventHandlers {
    query: OnceLock<Option<Query>>,
}

impl InjectionProvider for EventHandlers {
    fn id(&self) -> &str {
        "htmx.event_handlers"
    }

    fn hosts(&self) -> &[LanguageId] {
        static HOSTS: [LanguageId; 1] = [HTML];
        &HOSTS
    }

    fn detect(&self, ctx: &InjectionContext<'_>, out: &mut Vec<Injection>) {
        let Some(query) = self.query.get_or_init(|| Query::new(ctx.language, PATTERN).ok()) else {
            return;
        };
        let Some(value) = query.capture_index_for_name("value") else { return };
        ctx.for_each_match(query, |m| {
            for c in m.captures().iter().filter(|c| c.index == value) {
                out.push(Injection {
                    ranges: vec![c.node.byte_range()],
                    language: LanguageId::new_static("javascript"),
                    source: InjectionSource::Rule,
                });
            }
        });
    }
}
