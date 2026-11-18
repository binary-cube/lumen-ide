//! HTML as plain text, for what draws text only (a hover card, a tooltip, a list): blocks
//! on lines of their own, a table's rows as lines with their cells joined, list items
//! marked, the other tags taken off, entities as their characters. What each tag becomes
//! is a [`Rule`]: [`HtmlText::default`] knows HTML's usual ones, [`HtmlText::with`] adds or
//! changes one (a tag of its own, a different look); tags it doesn't know stay as written
//! (`array<int>` in a description is text, not a tag).

use std::sync::LazyLock;

/// What a tag becomes in text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Its content on lines of its own (a paragraph, a heading, a list).
    Block,
    /// A line break (`<br>`, `<hr>`).
    Break,
    /// A table's row: a line of its own, its cells joined.
    Row,
    /// A table's cell: after the separator when another came before it on its row.
    Cell,
    /// A list's item: on a line of its own, after the item's mark.
    Item,
    /// Taken off, its content kept (emphasis, links, code).
    Inline,
}

/// HTML's usual tags and what they become.
const RULES: &[(&str, Rule)] = &[
    ("p", Rule::Block),
    ("div", Rule::Block),
    ("pre", Rule::Block),
    ("blockquote", Rule::Block),
    ("ul", Rule::Block),
    ("ol", Rule::Block),
    ("dl", Rule::Block),
    ("dt", Rule::Block),
    ("dd", Rule::Block),
    ("h1", Rule::Block),
    ("h2", Rule::Block),
    ("h3", Rule::Block),
    ("h4", Rule::Block),
    ("h5", Rule::Block),
    ("h6", Rule::Block),
    ("table", Rule::Row),
    ("thead", Rule::Row),
    ("tbody", Rule::Row),
    ("tfoot", Rule::Row),
    ("tr", Rule::Row),
    ("td", Rule::Cell),
    ("th", Rule::Cell),
    ("li", Rule::Item),
    ("br", Rule::Break),
    ("hr", Rule::Break),
    ("a", Rule::Inline),
    ("abbr", Rule::Inline),
    ("b", Rule::Inline),
    ("code", Rule::Inline),
    ("em", Rule::Inline),
    ("i", Rule::Inline),
    ("kbd", Rule::Inline),
    ("small", Rule::Inline),
    ("span", Rule::Inline),
    ("strong", Rule::Inline),
    ("sub", Rule::Inline),
    ("sup", Rule::Inline),
    ("tt", Rule::Inline),
    ("u", Rule::Inline),
    ("var", Rule::Inline),
];

/// The converter: what each tag becomes, how cells are joined and items marked.
#[derive(Debug, Clone)]
pub struct HtmlText {
    rules: Vec<(String, Rule)>,
    cell_separator: String,
    item_mark: String,
}

impl Default for HtmlText {
    fn default() -> Self {
        Self {
            rules: RULES.iter().map(|(tag, rule)| ((*tag).to_owned(), *rule)).collect(),
            cell_separator: " · ".to_owned(),
            item_mark: "• ".to_owned(),
        }
    }
}

impl HtmlText {
    /// The same, tag `tag` (lowercase) becoming `rule` (added, or instead of HTML's usual).
    #[must_use]
    pub fn with(mut self, tag: &str, rule: Rule) -> Self {
        match self.rules.iter_mut().find(|(t, _)| t == tag) {
            Some(known) => known.1 = rule,
            None => self.rules.push((tag.to_owned(), rule)),
        }
        self
    }

    /// The same, a row's cells joined by `separator`.
    #[must_use]
    pub fn with_cell_separator(mut self, separator: &str) -> Self {
        separator.clone_into(&mut self.cell_separator);
        self
    }

    /// The same, a list's items after `mark`.
    #[must_use]
    pub fn with_item_mark(mut self, mark: &str) -> Self {
        mark.clone_into(&mut self.item_mark);
        self
    }

    /// What tag `name` (lowercase) becomes; `None`: not a tag it knows (written as is).
    pub fn rule(&self, name: &str) -> Option<Rule> {
        self.rules.iter().find(|(t, _)| t == name).map(|(_, r)| *r)
    }

    /// `html` as text.
    pub fn convert(&self, html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut rest = html;
        // A cell was written on the current row: the next one comes after the separator.
        let mut cell = false;
        while let Some(at) = rest.find(['<', '&']) {
            out.push_str(&rest[..at]);
            rest = &rest[at..];
            if rest.starts_with('&') {
                let (text, len) = entity(rest).unwrap_or(('&', 1));
                out.push(text);
                rest = &rest[len..];
                continue;
            }
            let Some((rule, closing, len)) = self.tag(rest) else {
                out.push('<');
                rest = &rest[1..];
                continue;
            };
            rest = &rest[len..];
            match rule {
                Rule::Cell if !closing => {
                    if cell {
                        out.push_str(&self.cell_separator);
                    }
                    cell = true;
                }
                Rule::Row => {
                    cell = false;
                    new_line(&mut out);
                }
                Rule::Item if !closing => {
                    new_line(&mut out);
                    out.push_str(&self.item_mark);
                }
                Rule::Block | Rule::Break => new_line(&mut out),
                Rule::Cell | Rule::Item | Rule::Inline => {}
            }
        }
        out.push_str(rest);
        out
    }

    /// A tag it knows at `text`'s start: its rule, whether it closes, its length.
    fn tag(&self, text: &str) -> Option<(Rule, bool, usize)> {
        let end = text.find('>')?;
        let inner = &text[1..end];
        let (closing, inner) = match inner.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, inner),
        };
        let len = inner.bytes().take_while(u8::is_ascii_alphanumeric).count();
        let ends_name = inner[len..].chars().next().is_none_or(|c| c.is_whitespace() || c == '/');
        if len == 0 || !ends_name {
            return None;
        }
        let rule = self.rule(&inner[..len].to_ascii_lowercase())?;
        Some((rule, closing, end + 1))
    }
}

/// `html` as text with HTML's usual rules.
pub fn to_text(html: &str) -> String {
    static DEFAULT: LazyLock<HtmlText> = LazyLock::new(HtmlText::default);
    DEFAULT.convert(html)
}

/// A new line in `out` unless it's at one (or empty).
fn new_line(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// An entity at `text`'s start (`&amp;`, `&#39;`, `&#x41;`): its character and length.
pub fn entity(text: &str) -> Option<(char, usize)> {
    let end = text.bytes().take(12).position(|b| b == b';')?;
    let body = &text[1..end];
    let c = match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        _ => {
            let number = body.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((c, end + 1))
}

#[cfg(test)]
#[path = "../tests/unit/text.rs"]
mod tests;
