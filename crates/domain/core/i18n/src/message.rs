//! ICU messages (the subset used in the UI): text, `{name}` arguments, `{n, plural, …}` with
//! `=0` / `one` / `few` / `other` and `#`, `{x, select, a {…} other {…}}`. A message is
//! parsed only once, when the catalog is loaded; formatting just walks it.

use crate::plural::{Locale, PluralCategory};

/// An argument's value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Num(f64),
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

macro_rules! numeric {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(n: $t) -> Self {
                Self::Num(n as f64)
            }
        }
    )*};
}
numeric!(u8, u16, i32, i64, u32, u64, usize, f32, f64);

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Text(String),
    Arg(String),
    /// `#` in a plural case: the formatted number.
    Hash,
    Plural {
        arg: String,
        cases: Vec<(Selector, Vec<Node>)>,
    },
    Select {
        arg: String,
        cases: Vec<(String, Vec<Node>)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Selector {
    Exact(f64),
    Category(PluralCategory),
}

/// A parsed message.
#[derive(Debug, Clone, PartialEq)]
pub struct Message(Vec<Node>);

impl Message {
    /// Parses a message; on invalid syntax, the text is kept as is (a wrong
    /// message in a catalog doesn't break the UI).
    pub fn parse(source: &str) -> Self {
        let mut parser = Parser { chars: source.chars().collect(), pos: 0 };
        match parser.nodes(false) {
            Some(nodes) if parser.pos == parser.chars.len() => Self(nodes),
            _ => Self(vec![Node::Text(source.to_owned())]),
        }
    }

    pub fn format(&self, locale: Locale, args: &[(&str, Value)]) -> String {
        let mut out = String::new();
        write_nodes(&self.0, locale, args, None, &mut out);
        out
    }
}

fn arg<'a>(args: &'a [(&str, Value)], name: &str) -> Option<&'a Value> {
    args.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
}

fn write_nodes(
    nodes: &[Node],
    locale: Locale,
    args: &[(&str, Value)],
    number: Option<f64>,
    out: &mut String,
) {
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Hash => match number {
                Some(n) => out.push_str(&locale.format_number(n)),
                None => out.push('#'),
            },
            Node::Arg(name) => match arg(args, name) {
                Some(Value::Str(s)) => out.push_str(s),
                Some(Value::Num(n)) => out.push_str(&locale.format_number(*n)),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            },
            Node::Plural { arg: name, cases } => {
                let n = match arg(args, name) {
                    Some(Value::Num(n)) => *n,
                    Some(Value::Str(s)) => s.parse().unwrap_or(0.0),
                    None => 0.0,
                };
                let category = locale.plural(n);
                let chosen = cases
                    .iter()
                    .find(|(s, _)| *s == Selector::Exact(n))
                    .or_else(|| cases.iter().find(|(s, _)| *s == Selector::Category(category)))
                    .or_else(|| {
                        cases.iter().find(|(s, _)| *s == Selector::Category(PluralCategory::Other))
                    });
                if let Some((_, nodes)) = chosen {
                    write_nodes(nodes, locale, args, Some(n), out);
                }
            }
            Node::Select { arg: name, cases } => {
                let value = match arg(args, name) {
                    Some(Value::Str(s)) => s.clone(),
                    Some(Value::Num(n)) => locale.format_number(*n),
                    None => String::new(),
                };
                let chosen = cases
                    .iter()
                    .find(|(k, _)| *k == value)
                    .or_else(|| cases.iter().find(|(k, _)| k == "other"));
                if let Some((_, nodes)) = chosen {
                    write_nodes(nodes, locale, args, number, out);
                }
            }
        }
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn word(&mut self) -> String {
        self.skip_space();
        let start = self.pos;
        while self.peek().is_some_and(|c| !c.is_whitespace() && !matches!(c, ',' | '{' | '}')) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn expect(&mut self, c: char) -> Option<()> {
        self.skip_space();
        (self.peek() == Some(c)).then(|| self.pos += 1)
    }

    /// The nodes up to `}` (if `nested`) or up to the end.
    fn nodes(&mut self, nested: bool) -> Option<Vec<Node>> {
        let mut nodes = Vec::new();
        let mut text = String::new();
        while let Some(c) = self.peek() {
            match c {
                '}' if nested => break,
                '{' => {
                    if !text.is_empty() {
                        nodes.push(Node::Text(std::mem::take(&mut text)));
                    }
                    self.pos += 1;
                    nodes.push(self.placeholder()?);
                }
                '#' if nested => {
                    if !text.is_empty() {
                        nodes.push(Node::Text(std::mem::take(&mut text)));
                    }
                    self.pos += 1;
                    nodes.push(Node::Hash);
                }
                // `''` = apostrophe; `'{…}'` = literal text (as in ICU).
                '\'' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('\'') => {
                            text.push('\'');
                            self.pos += 1;
                        }
                        Some('{' | '}' | '#') => {
                            while let Some(c) = self.peek() {
                                self.pos += 1;
                                if c == '\'' {
                                    break;
                                }
                                text.push(c);
                            }
                        }
                        _ => text.push('\''),
                    }
                }
                c => {
                    text.push(c);
                    self.pos += 1;
                }
            }
        }
        if !text.is_empty() {
            nodes.push(Node::Text(text));
        }
        Some(nodes)
    }

    /// After `{`: `name}`, `name, plural, …}` or `name, select, …}`.
    fn placeholder(&mut self) -> Option<Node> {
        let name = self.word();
        self.skip_space();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Some(Node::Arg(name));
        }
        self.expect(',')?;
        let kind = self.word();
        self.expect(',')?;
        match kind.as_str() {
            "plural" => {
                let mut cases = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek() == Some('}') {
                        self.pos += 1;
                        break;
                    }
                    let key = self.word();
                    let selector = match key.strip_prefix('=') {
                        Some(n) => Selector::Exact(n.parse().ok()?),
                        None => Selector::Category(PluralCategory::parse(&key)?),
                    };
                    self.expect('{')?;
                    let nodes = self.nodes(true)?;
                    self.expect('}')?;
                    cases.push((selector, nodes));
                }
                Some(Node::Plural { arg: name, cases })
            }
            "select" => {
                let mut cases = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek() == Some('}') {
                        self.pos += 1;
                        break;
                    }
                    let key = self.word();
                    self.expect('{')?;
                    let nodes = self.nodes(true)?;
                    self.expect('}')?;
                    cases.push((key, nodes));
                }
                Some(Node::Select { arg: name, cases })
            }
            _ => None,
        }
    }
}
