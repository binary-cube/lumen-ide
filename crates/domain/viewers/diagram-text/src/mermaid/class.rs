//! `classDiagram`: classes (generics, labels, annotations like `<<interface>>`), their
//! members (in a `{ … }` body or as `Class : member`), relations (inheritance,
//! composition, aggregation, association, dependency, realization, links; both ends,
//! cardinalities, labels), notes, namespaces, `direction`, styles and `click` links.

use std::collections::HashMap;

use super::{Direction, Error, ErrorKind, Line, Style, label, unquote};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassDiagram {
    pub direction: Direction,
    pub classes: Vec<Class>,
    pub relations: Vec<Relation>,
    pub notes: Vec<Note>,
    /// Each namespace: its name and its classes.
    pub namespaces: Vec<(String, Vec<usize>)>,
}

/// A member: its text as written (visibility and type included), and how it's shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Member {
    pub text: String,
    /// `$`: underlined.
    pub is_static: bool,
    /// `*`: in italics.
    pub is_abstract: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Class {
    pub id: String,
    /// What it shows (its generic as `<T>`).
    pub label: String,
    pub annotation: Option<String>,
    pub attributes: Vec<Member>,
    pub methods: Vec<Member>,
    pub style: Style,
    pub link: Option<String>,
}

/// What a relation's end looks like.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Mark {
    #[default]
    None,
    /// `<|` / `|>`
    Inheritance,
    /// `*`
    Composition,
    /// `o`
    Aggregation,
    /// `<` / `>`
    Arrow,
    /// `()`
    Lollipop,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Relation {
    pub from: usize,
    pub to: usize,
    pub from_mark: Mark,
    pub to_mark: Mark,
    pub dashed: bool,
    pub label: Option<String>,
    pub from_card: Option<String>,
    pub to_card: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    /// The class it's for (none: the diagram's).
    pub class: Option<usize>,
    pub text: String,
}

#[derive(Default)]
struct State {
    d: ClassDiagram,
    ids: HashMap<String, usize>,
    classes: HashMap<String, Style>,
    given: Vec<(usize, String)>,
    /// The body being read (`class A {`): its class and its line.
    body: Option<(usize, usize)>,
    namespace: Option<(usize, usize)>,
}

pub(super) fn parse(lines: &[Line<'_>]) -> Result<ClassDiagram, Error> {
    let mut st = State::default();
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if let Some((class, _)) = st.body {
            if t == "}" {
                st.body = None;
            } else if let Some(a) = t.strip_prefix("<<").and_then(|a| a.strip_suffix(">>")) {
                st.d.classes[class].annotation = Some(a.trim().to_owned());
            } else {
                st.member(class, t);
            }
            continue;
        }
        st.statement(line)?;
    }
    if let Some((_, number)) = st.body {
        return Err(Error::new(number, ErrorKind::Unclosed("}")));
    }
    if let Some((_, number)) = st.namespace {
        return Err(Error::new(number, ErrorKind::Unclosed("}")));
    }
    for (c, name) in &st.given {
        if let Some(style) = st.classes.get(name) {
            let class = &mut st.d.classes[*c];
            class.style = class.style.merged(style);
        }
    }
    Ok(st.d)
}

impl State {
    /// The class named `id` (`Name~T~`: generic), made if it's new.
    fn class(&mut self, id: &str) -> usize {
        let id = id.trim();
        let name = id.split('~').next().unwrap_or(id);
        let shown = generic(id);
        if let Some(&c) = self.ids.get(name) {
            if id.contains('~') {
                self.d.classes[c].label = shown;
            }
            return c;
        }
        let c = self.d.classes.len();
        let label = shown;
        self.d.classes.push(Class { id: name.to_owned(), label, ..Class::default() });
        self.ids.insert(name.to_owned(), c);
        if let Some((ns, _)) = self.namespace {
            self.d.namespaces[ns].1.push(c);
        }
        c
    }

    fn member(&mut self, class: usize, text: &str) {
        let mut text = text.trim().to_owned();
        let mut m = Member::default();
        // The classifier may follow the parameters (`run()* int`).
        for (mark, flag) in [(")*", 0), (")$", 1)] {
            if text.contains(mark) {
                text = text.replacen(mark, ")", 1);
                if flag == 0 {
                    m.is_abstract = true;
                } else {
                    m.is_static = true;
                }
            }
        }
        loop {
            if let Some(t) = text.strip_suffix('$') {
                m.is_static = true;
                text = t.trim_end().to_owned();
            } else if let Some(t) = text.strip_suffix('*') {
                m.is_abstract = true;
                text = t.trim_end().to_owned();
            } else {
                break;
            }
        }
        // `List~int~` reads as `List<int>`.
        m.text = generic(&text);
        let class = &mut self.d.classes[class];
        if m.text.contains('(') {
            class.methods.push(m);
        } else {
            class.attributes.push(m);
        }
    }

    fn statement(&mut self, line: &Line<'_>) -> Result<(), Error> {
        let t = line.text.trim();
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "direction" => {
                self.d.direction = Direction::parse(rest).ok_or_else(|| Error::unexpected(line))?;
                return Ok(());
            }
            "class" => {
                let (head, open) = match rest.strip_suffix('{') {
                    Some(h) => (h.trim(), true),
                    None => (rest, false),
                };
                let (head, class_name) = match head.split_once(":::") {
                    Some((h, c)) => (h.trim(), Some(c.trim())),
                    None => (head, None),
                };
                let (id, shown) = match head.split_once('[') {
                    Some((id, l)) => (id.trim(), Some(label(l.trim_end_matches(']')))),
                    None => (head, None),
                };
                let c = self.class(id);
                if let Some(shown) = shown {
                    self.d.classes[c].label = shown;
                }
                if let Some(name) = class_name {
                    self.given.push((c, name.to_owned()));
                }
                if open {
                    self.body = Some((c, line.number));
                }
                return Ok(());
            }
            "namespace" => {
                let name = rest.trim_end_matches('{').trim().to_owned();
                self.d.namespaces.push((name, Vec::new()));
                self.namespace = Some((self.d.namespaces.len() - 1, line.number));
                return Ok(());
            }
            "}" if self.namespace.is_some() => {
                self.namespace = None;
                return Ok(());
            }
            "note" => {
                let (class, text) = match rest.strip_prefix("for ") {
                    Some(r) => {
                        let (id, text) =
                            r.trim().split_once(char::is_whitespace).unwrap_or((r, ""));
                        (Some(self.class(id)), text)
                    }
                    None => (None, rest),
                };
                self.d.notes.push(Note { class, text: label(text.trim()) });
                return Ok(());
            }
            "classDef" => {
                let (names, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                for name in names.split(',') {
                    self.classes.entry(name.trim().to_owned()).or_default().apply(style);
                }
                return Ok(());
            }
            "cssClass" => {
                let parts: Vec<&str> = rest.split('"').collect();
                if let (Some(ids), Some(name)) = (parts.get(1), parts.get(2)) {
                    for id in ids.split(',') {
                        let c = self.class(id);
                        self.given.push((c, name.trim().to_owned()));
                    }
                }
                return Ok(());
            }
            "style" => {
                let (id, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let c = self.class(id);
                let s = Style::parse(style);
                let class = &mut self.d.classes[c];
                class.style = class.style.merged(&s);
                return Ok(());
            }
            "click" | "link" => {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let id = parts.next().unwrap_or_default();
                let r = parts.next().unwrap_or_default().trim();
                let r = r.strip_prefix("href").map(str::trim).unwrap_or(r);
                if let Some(url) = r.split('"').nth(1) {
                    let c = self.class(id);
                    self.d.classes[c].link = Some(url.to_owned());
                }
                return Ok(());
            }
            "callback" => return Ok(()),
            _ => {}
        }
        if let Some(a) = t.strip_prefix("<<")
            && let Some((annotation, id)) = a.split_once(">>")
        {
            let c = self.class(id);
            self.d.classes[c].annotation = Some(annotation.trim().to_owned());
            return Ok(());
        }
        if let Some(relation) = self.relation(t) {
            self.d.relations.push(relation);
            return Ok(());
        }
        // `Class : member`.
        if let Some((id, member)) = t.split_once(':')
            && !id.trim().is_empty()
            && !id.trim().contains(char::is_whitespace)
        {
            let c = self.class(id);
            self.member(c, member);
            return Ok(());
        }
        // A class named alone.
        if t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '~') {
            self.class(t);
            return Ok(());
        }
        Err(Error::unexpected(line))
    }

    /// `A "1" <|-- "many" B : label`.
    fn relation(&mut self, t: &str) -> Option<Relation> {
        let (body, label_text) = match t.split_once(" : ").or_else(|| t.split_once(':')) {
            Some((b, l)) if !b.contains('"') || b.matches('"').count() % 2 == 0 => {
                (b.trim(), Some(l.trim()))
            }
            _ => (t, None),
        };
        // The link in the middle: a run of `-` or `.` (two at least).
        let bytes = body.as_bytes();
        let mut i = 0;
        let (mut start, mut end) = (None, 0);
        while i < bytes.len() {
            if bytes[i] == b'"' {
                i += body[i + 1..].find('"').map_or(bytes.len(), |e| e + 2);
                continue;
            }
            if (bytes[i] == b'-' || bytes[i] == b'.') && bytes.get(i + 1) == Some(&bytes[i]) {
                let s = i;
                while i < bytes.len() && bytes[i] == bytes[s] {
                    i += 1;
                }
                start = Some(s);
                end = i;
                break;
            }
            i += 1;
        }
        let start = start?;
        let dashed = bytes[start] == b'.';
        let left = &body[..start];
        let right = &body[end..];
        let (left, from_mark) = strip_mark_end(left);
        let (right, to_mark) = strip_mark_start(right);
        let (from_id, from_card) = split_card(left, true);
        let (to_id, to_card) = split_card(right, false);
        if from_id.is_empty() || to_id.is_empty() || from_id.contains(' ') || to_id.contains(' ') {
            return None;
        }
        let from = self.class(from_id);
        let to = self.class(to_id);
        Some(Relation {
            from,
            to,
            from_mark,
            to_mark,
            dashed,
            label: label_text.filter(|l| !l.is_empty()).map(label),
            from_card,
            to_card,
        })
    }
}

/// `List~int~` as `List<int>`.
fn generic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut open = false;
    for c in s.chars() {
        if c == '~' {
            out.push(if open { '>' } else { '<' });
            open = !open;
        } else {
            out.push(c);
        }
    }
    out
}

/// The mark at the end of a relation's left side (`A <|`, `A *`).
fn strip_mark_end(s: &str) -> (&str, Mark) {
    let s = s.trim_end();
    for (m, mark) in [
        ("<|", Mark::Inheritance),
        ("()", Mark::Lollipop),
        ("*", Mark::Composition),
        ("o", Mark::Aggregation),
        ("<", Mark::Arrow),
    ] {
        if let Some(rest) = s.strip_suffix(m)
            // `o` only as a mark when it stands apart from the name (`A o--`).
            && (m != "o" || rest.ends_with([' ', '"']))
        {
            return (rest, mark);
        }
    }
    (s, Mark::None)
}

/// The mark at the start of a relation's right side (`|> B`, `* B`).
fn strip_mark_start(s: &str) -> (&str, Mark) {
    let s = s.trim_start();
    for (m, mark) in [
        ("|>", Mark::Inheritance),
        ("()", Mark::Lollipop),
        ("*", Mark::Composition),
        ("o", Mark::Aggregation),
        (">", Mark::Arrow),
    ] {
        if let Some(rest) = s.strip_prefix(m)
            && (m != "o" || rest.starts_with([' ', '"']))
        {
            return (rest, mark);
        }
    }
    (s, Mark::None)
}

/// A side's class and its cardinality (`A "1"` on the left, `"*" B` on the right).
fn split_card(s: &str, left: bool) -> (&str, Option<String>) {
    let s = s.trim();
    if left {
        if let Some(body) = s.strip_suffix('"')
            && let Some(q) = body.rfind('"')
        {
            return (body[..q].trim(), Some(unquote(&s[q..]).to_owned()));
        }
    } else if let Some(body) = s.strip_prefix('"')
        && let Some(q) = body.find('"')
    {
        return (body[q + 1..].trim(), Some(body[..q].to_owned()));
    }
    (s, None)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/class.rs"]
mod tests;
