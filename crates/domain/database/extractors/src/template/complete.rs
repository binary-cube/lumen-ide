//! What a template's editor offers at the cursor, inside a `{{ }}` tag: the fields, the
//! sections after `#` / `^` (and the one to close after `/`), the filters after `|`.

use std::ops::Range;

/// What a suggestion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    Field,
    Section,
    Filter,
}

/// A suggestion: what's shown, what's put in place of the word typed, what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub detail: String,
    pub kind: SuggestionKind,
}

const FIELDS: &[(&str, &str)] = &[
    ("value", "extract.complete_value"),
    ("column.name", "extract.complete_column_name"),
    ("column.type", "extract.complete_column_type"),
    ("column.kind", "extract.complete_column_kind"),
    ("column.index", "extract.complete_column_index"),
    ("table", "extract.complete_table"),
    ("row.index", "extract.complete_row_index"),
    ("row.number", "extract.complete_row_number"),
];

const SECTIONS: &[(&str, &str)] = &[
    ("rows", "extract.complete_rows"),
    ("columns", "extract.complete_columns"),
    ("first", "extract.complete_first"),
    ("last", "extract.complete_last"),
    ("null", "extract.complete_null"),
    ("number", "extract.complete_number"),
    ("key", "extract.complete_key"),
];

const FILTERS: &[(&str, &str)] = &[
    ("sql", "extract.complete_sql"),
    ("json", "extract.complete_json"),
    ("csv", "extract.complete_csv"),
    ("html", "extract.complete_html"),
    ("xml", "extract.complete_xml"),
    ("md", "extract.complete_md"),
    ("php", "extract.complete_php"),
    ("py", "extract.complete_py"),
    ("upper", "extract.complete_upper"),
    ("lower", "extract.complete_lower"),
    ("trim", "extract.complete_trim"),
    ("pad:", "extract.complete_pad"),
];

/// The sections open before `at` (innermost last), by their names.
fn open_sections(text: &str, at: usize) -> Vec<&str> {
    let mut open = Vec::new();
    let mut rest = &text[..at];
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let inner = after[..end].trim();
        if let Some(name) = inner.strip_prefix('#').or_else(|| inner.strip_prefix('^')) {
            open.push(name.trim());
        } else if let Some(name) = inner.strip_prefix('/')
            && let Some(i) = open.iter().rposition(|o| *o == name.trim())
        {
            open.truncate(i);
        }
        rest = &after[end + 2..];
    }
    open
}

/// What's offered at byte `offset` of `text`, and the range it replaces (`None`: the cursor
/// isn't in a tag).
pub fn complete(text: &str, offset: usize) -> Option<(Range<usize>, Vec<Suggestion>)> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let before = &text[..offset];
    let tag = before.rfind("{{")?;
    if before[tag..].contains("}}") || before[..tag].ends_with('\\') {
        return None;
    }
    let inner_start = tag + 2;
    let inner = &text[inner_start..offset];
    if inner.starts_with('!') {
        return None;
    }
    // The word being typed (letters, digits, `.`, `_`, `:`, `-`).
    let word_len = inner
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
        .map(char::len_utf8)
        .sum::<usize>();
    let replace = offset - word_len..offset;
    let typed = &text[replace.clone()];
    let head = inner[..inner.len() - word_len].trim_start();
    let make = |list: &[(&str, &str)], kind: SuggestionKind| -> Vec<Suggestion> {
        list.iter()
            .filter(|(name, _)| name.starts_with(typed) || typed.is_empty())
            .map(|(name, key)| Suggestion {
                label: (*name).to_owned(),
                insert: (*name).to_owned(),
                detail: i18n::t(key),
                kind,
            })
            .collect()
    };
    let items = if head.contains('|') {
        make(FILTERS, SuggestionKind::Filter)
    } else if head == "#" || head == "^" {
        let list: Vec<(&str, &str)> = SECTIONS
            .iter()
            .copied()
            .filter(|(name, _)| head == "#" || !matches!(*name, "rows" | "columns"))
            .collect();
        make(&list, SuggestionKind::Section)
    } else if head == "/" {
        // The innermost open section first.
        let open = open_sections(text, tag);
        let mut list: Vec<(&str, &str)> = SECTIONS.to_vec();
        if let Some(innermost) = open.last() {
            list.sort_by_key(|(name, _)| *name != *innermost);
        }
        make(&list, SuggestionKind::Section)
    } else if head.is_empty() {
        make(FIELDS, SuggestionKind::Field)
    } else {
        Vec::new()
    };
    (!items.is_empty()).then_some((replace, items))
}
