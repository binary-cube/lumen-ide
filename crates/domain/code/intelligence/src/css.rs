//! How the languages that hold CSS refer to its classes, ids and custom properties: a
//! name found in a file is the symbol for sure, maybe, or something else, by its place.
//! (The selectors themselves are read by the `stylesheet` adapter.)

use crate::SymbolKind;

/// A CSS symbol: a class, an id, a custom property, a mixin.
pub fn is_css_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::CssClass | SymbolKind::CssId | SymbolKind::CssVariable | SymbolKind::Mixin
    )
}

/// What a name found in a file is, by its place alone (CSS's names: no definitions say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceMatch {
    /// The symbol for sure (a selector, a `class` attribute…).
    Sure,
    /// Maybe (a string in a script).
    Maybe,
    /// Something else (text between tags, `#card` for the class `card`).
    Other,
}

/// How a language's files refer to CSS classes and ids (given by its module, see
/// [`crate::LanguageIntelligence::css_references`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CssReferences {
    /// A stylesheet: selectors are declared and used as such.
    pub stylesheet: bool,
    /// The attributes whose value lists classes (`class="`), ids (`id="`).
    pub class_attributes: &'static [&'static str],
    pub id_attributes: &'static [&'static str],
    /// The calls taking a class name (`classList.add(`).
    pub class_calls: &'static [&'static str],
    /// A script: its strings may be class names (unsure).
    pub script: bool,
}

/// A CSS class / id / custom property found by name, in a file whose language refers to
/// them as `refs` says (`None`: unknown): sure in stylesheets and in `class` / `id`
/// attributes, unsure elsewhere (a string in a script may or may not be it).
pub fn css_match(
    text: &str,
    range: &std::ops::Range<usize>,
    kind: SymbolKind,
    refs: Option<&CssReferences>,
) -> PlaceMatch {
    let refs = refs.copied().unwrap_or_default();
    let before = text[..range.start].chars().next_back();
    if refs.stylesheet {
        return match (kind, before) {
            (SymbolKind::CssClass, Some('.')) | (SymbolKind::CssId, Some('#')) => PlaceMatch::Sure,
            (SymbolKind::CssVariable | SymbolKind::Mixin, _) => PlaceMatch::Sure,
            // `.card` searched, `#card` found (or a property value): another thing.
            _ => PlaceMatch::Other,
        };
    }
    // In markup: inside a `class="…"` / `id="…"` attribute value.
    let line_start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..range.start];
    let attribute = match kind {
        SymbolKind::CssClass => refs.class_attributes,
        SymbolKind::CssId => refs.id_attributes,
        _ => &[],
    };
    let in_attribute = attribute.iter().any(|a| {
        line.rfind(a).is_some_and(|i| {
            let value = &line[i + a.len()..];
            !value.contains(a.chars().last().unwrap_or('"'))
        })
    });
    let selector = match kind {
        SymbolKind::CssClass => before == Some('.'),
        SymbolKind::CssId => before == Some('#'),
        _ => false,
    };
    let script = refs.script;
    let quoted = line.matches('"').count() % 2 == 1 || line.matches('\'').count() % 2 == 1;
    // `el.classList.add('card')`: a class for sure.
    let class_list = kind == SymbolKind::CssClass
        && quoted
        && refs
            .class_calls
            .iter()
            .any(|call| line.trim_end_matches(['\'', '"', '`']).ends_with(call));
    if in_attribute || selector || class_list {
        // A selector in a `<style>` block or a query (`querySelector('.card')`) is sure.
        PlaceMatch::Sure
    } else if quoted || script {
        // Some string: maybe built into a class list, maybe not.
        PlaceMatch::Maybe
    } else {
        // Plain text between tags.
        PlaceMatch::Other
    }
}

/// [`css_match`] for CSS's names; `None` for the other kinds (resolved through their
/// definitions).
pub fn place_match(
    text: &str,
    range: &std::ops::Range<usize>,
    kind: SymbolKind,
    refs: Option<&CssReferences>,
) -> Option<PlaceMatch> {
    is_css_kind(kind).then(|| css_match(text, range, kind, refs))
}

#[cfg(test)]
#[path = "../tests/unit/css.rs"]
mod tests;
