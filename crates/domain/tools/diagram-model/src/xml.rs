//! The XML of a `.drawio` file as a tree that keeps what it read: each element's attributes
//! in their order, each value with the text it was written as (written back as it was while
//! unchanged: entities, numbers, quotes), the elements it doesn't know whole. Written the way
//! draw.io writes it (two spaces a level, `<empty />`), so a file saved without a change is the
//! same file, and one change is one line of a diff.

use crate::Error;

/// How deep elements may nest (draw.io's go five or six levels down).
pub(crate) const MAX_DEPTH: usize = 64;

/// An attribute: its name, its value, and the text it was written as (`None`: set here,
/// escaped when written).
#[derive(Debug, Clone, PartialEq)]
struct Attr {
    name: String,
    value: String,
    raw: Option<Box<str>>,
}

/// An element, its attributes in order, its children; the text inside it when it has no
/// children (a compressed page's).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    attrs: Vec<Attr>,
    pub children: Vec<Element>,
    pub text: Option<String>,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Self::default() }
    }

    /// What it holds in memory, roughly (its names, values and texts, its children's).
    pub(crate) fn weight(&self) -> usize {
        let attrs: usize = self
            .attrs
            .iter()
            .map(|a| 48 + a.name.len() + a.value.len() + a.raw.as_ref().map_or(0, |r| r.len()))
            .sum();
        let children: usize = self.children.iter().map(Element::weight).sum();
        96 + self.name.len() + attrs + children + self.text.as_ref().map_or(0, String::len)
    }

    /// The value of attribute `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.name == name).map(|a| a.value.as_str())
    }

    /// The attributes, in order.
    pub fn attrs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str()))
    }

    /// Attribute `name` set to `value`: in its place if it's there (its text written anew
    /// only if the value changed), else after the others.
    pub fn set_attr(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attrs.iter_mut().find(|a| a.name == name) {
            Some(a) if a.value == value => {}
            Some(a) => {
                a.value = value;
                a.raw = None;
            }
            None => self.attrs.push(Attr { name: name.to_owned(), value, raw: None }),
        }
    }

    /// Attribute `name` taken away.
    pub fn remove_attr(&mut self, name: &str) {
        self.attrs.retain(|a| a.name != name);
    }

    /// The first child named `name` whose `as` is `as_` (`None`: any).
    pub fn child(&self, name: &str, as_: Option<&str>) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name && (as_.is_none() || c.attr("as") == as_))
    }

    pub fn child_mut(&mut self, name: &str, as_: Option<&str>) -> Option<&mut Element> {
        self.children.iter_mut().find(|c| c.name == name && (as_.is_none() || c.attr("as") == as_))
    }

    /// `node`'s element (read from `source`), `count` the elements read so far (at most
    /// `limit`), `depth` how deep it is.
    pub(crate) fn read(
        node: roxmltree::Node<'_, '_>,
        source: &str,
        count: &mut usize,
        limit: usize,
        depth: usize,
    ) -> Result<Self, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        *count += 1;
        if *count > limit {
            return Err(Error::TooManyCells);
        }
        let attrs = node
            .attributes()
            .map(|a| {
                let value = a.value();
                // Kept as written only when it can go back between double quotes (a value
                // written between single ones may hold a bare `"`), and only when writing
                // the value wouldn't give it back the same (no character to escape: ids,
                // numbers, styles — most of a file — keep one copy).
                let plain = |raw: &str| {
                    raw == value
                        && !value.contains(|c: char| {
                            matches!(c, '&' | '"' | '\'' | '<' | '>') || c.is_control()
                        })
                };
                let raw =
                    source.get(a.range_value()).filter(|raw| !raw.contains('"') && !plain(raw));
                Attr {
                    name: a.name().to_owned(),
                    value: value.to_owned(),
                    raw: raw.map(Into::into),
                }
            })
            .collect();
        let mut children = Vec::new();
        let mut text = String::new();
        for child in node.children() {
            if child.is_element() {
                children.push(Self::read(child, source, count, limit, depth + 1)?);
            } else if child.is_text() {
                text.push_str(child.text().unwrap_or_default());
            }
        }
        let text = (children.is_empty() && !text.trim().is_empty()).then(|| text.trim().to_owned());
        Ok(Self { name: node.tag_name().name().to_owned(), attrs, children, text })
    }

    /// Written at `depth` (two spaces a level), as draw.io writes it.
    pub fn write(&self, out: &mut String, depth: usize) {
        indent(out, depth);
        self.write_open(out);
        if self.children.is_empty() {
            match &self.text {
                Some(text) => {
                    out.push('>');
                    escape_text(text, out);
                    out.push_str("</");
                    out.push_str(&self.name);
                    out.push_str(">\n");
                }
                None => out.push_str(" />\n"),
            }
            return;
        }
        out.push_str(">\n");
        for child in &self.children {
            child.write(out, depth + 1);
        }
        self.write_close(out, depth);
    }

    /// `<name a="…"` (not closed).
    pub(crate) fn write_open(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for a in &self.attrs {
            out.push(' ');
            out.push_str(&a.name);
            out.push_str("=\"");
            match &a.raw {
                Some(raw) => out.push_str(raw),
                None => escape_attr(&a.value, out),
            }
            out.push('"');
        }
    }

    /// `</name>` at `depth`.
    pub(crate) fn write_close(&self, out: &mut String, depth: usize) {
        indent(out, depth);
        out.push_str("</");
        out.push_str(&self.name);
        out.push_str(">\n");
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// An attribute's value as draw.io escapes it (`mxUtils.htmlEntities`, line breaks kept as
/// `&#xa;`).
fn escape_attr(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("&#xa;"),
            // Kept as they are (read back, a bare one would be a space).
            '\t' => out.push_str("&#x9;"),
            '\r' => out.push_str("&#xd;"),
            // Not allowed in XML at all: left out, the file stays readable.
            c if c.is_control() && (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
}

fn escape_text(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
}

/// A number as draw.io writes it (JavaScript's: `340`, `12.5`, never `340.0`).
pub fn number(value: f64) -> String {
    // Never `inf` / `NaN` in a file: draw.io couldn't read it.
    if !value.is_finite() {
        return "0".into();
    }
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        // Whole and in range: exact as an integer.
        #[allow(clippy::cast_possible_truncation)]
        let whole = value as i64;
        return whole.to_string();
    }
    format!("{value}")
}

#[cfg(test)]
#[path = "../tests/unit/xml.rs"]
mod tests;
