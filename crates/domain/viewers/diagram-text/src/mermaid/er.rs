//! `erDiagram`: entities (aliases `A["Label"]`) with their attributes (type, name, keys
//! `PK` / `FK` / `UK`, a comment) and relationships with both ends' cardinality
//! (`||`, `|o`, `}o`, `}|` — or their words), identifying (`--`) or not (`..`), labeled.

use std::collections::HashMap;

use super::{Direction, Error, ErrorKind, Line, label, unquote};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ErDiagram {
    pub direction: Direction,
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attribute {
    pub kind: String,
    pub name: String,
    pub keys: Vec<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entity {
    pub id: String,
    pub label: String,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Cardinality {
    ZeroOrOne,
    #[default]
    ExactlyOne,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relation {
    pub from: usize,
    pub to: usize,
    pub from_card: Cardinality,
    pub to_card: Cardinality,
    /// `--`: the child can't exist without the parent.
    pub identifying: bool,
    pub label: String,
}

pub(super) fn parse(lines: &[Line<'_>]) -> Result<ErDiagram, Error> {
    let mut d = ErDiagram::default();
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut body: Option<(usize, usize)> = None;
    let entity = |d: &mut ErDiagram, ids: &mut HashMap<String, usize>, name: &str| -> usize {
        let name = name.trim();
        let (id, alias) = match name.split_once('[') {
            Some((id, alias)) => (unquote(id.trim()), Some(label(alias.trim_end_matches(']')))),
            None => (unquote(name), None),
        };
        let e = match ids.get(id) {
            Some(&e) => e,
            None => {
                d.entities.push(Entity {
                    id: id.to_owned(),
                    label: id.to_owned(),
                    attributes: Vec::new(),
                });
                ids.insert(id.to_owned(), d.entities.len() - 1);
                d.entities.len() - 1
            }
        };
        if let Some(alias) = alias {
            d.entities[e].label = alias;
        }
        e
    };
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if let Some((e, _)) = body {
            if t == "}" {
                body = None;
                continue;
            }
            d.entities[e].attributes.push(attribute(t).ok_or_else(|| Error::unexpected(line))?);
            continue;
        }
        if let Some(rest) = t.strip_prefix("direction") {
            d.direction = Direction::parse(rest).ok_or_else(|| Error::unexpected(line))?;
            continue;
        }
        if let Some(name) = t.strip_suffix('{') {
            let e = entity(&mut d, &mut ids, name);
            body = Some((e, line.number));
            continue;
        }
        if let Some((a, b, from_card, to_card, identifying, text)) = relation(t) {
            let from = entity(&mut d, &mut ids, a);
            let to = entity(&mut d, &mut ids, b);
            d.relations.push(Relation { from, to, from_card, to_card, identifying, label: text });
            continue;
        }
        if !t.contains(char::is_whitespace) || t.starts_with('"') {
            entity(&mut d, &mut ids, t);
            continue;
        }
        return Err(Error::unexpected(line));
    }
    if let Some((_, number)) = body {
        return Err(Error::new(number, ErrorKind::Unclosed("}")));
    }
    Ok(d)
}

/// `string name PK, FK "a comment"`.
fn attribute(t: &str) -> Option<Attribute> {
    let (main, comment) = match t.find('"') {
        Some(q) => (&t[..q], Some(unquote(&t[q..]).to_owned())),
        None => (t, None),
    };
    let mut words = main.split_whitespace();
    let kind = words.next()?.replace('~', "<");
    let name = words.next()?.to_owned();
    let keys =
        words.flat_map(|w| w.split(',')).filter(|k| !k.is_empty()).map(str::to_owned).collect();
    Some(Attribute { kind, name, keys, comment })
}

type Parsed<'a> = (&'a str, &'a str, Cardinality, Cardinality, bool, String);

/// `A ||--o{ B : label`.
fn relation(t: &str) -> Option<Parsed<'_>> {
    let (body, text) =
        t.split_once(':').map(|(b, l)| (b.trim(), label(l.trim()))).unwrap_or((t, String::new()));
    let (identifying, at) = match (body.find("--"), body.find("..")) {
        (Some(i), _) => (true, i),
        (None, Some(i)) => (false, i),
        (None, None) => return None,
    };
    let left = body[..at].trim_end();
    let right = body[at + 2..].trim_start();
    // The symbols right next to the link.
    let (a, from_card) = left_card(left)?;
    let (b, to_card) = right_card(right)?;
    Some((a.trim(), b.trim(), from_card, to_card, identifying, text))
}

fn left_card(s: &str) -> Option<(&str, Cardinality)> {
    for (m, c) in [
        ("|o", Cardinality::ZeroOrOne),
        ("||", Cardinality::ExactlyOne),
        ("}o", Cardinality::ZeroOrMore),
        ("}|", Cardinality::OneOrMore),
    ] {
        if let Some(rest) = s.strip_suffix(m) {
            return Some((rest, c));
        }
    }
    None
}

fn right_card(s: &str) -> Option<(&str, Cardinality)> {
    for (m, c) in [
        ("o|", Cardinality::ZeroOrOne),
        ("||", Cardinality::ExactlyOne),
        ("o{", Cardinality::ZeroOrMore),
        ("|{", Cardinality::OneOrMore),
    ] {
        if let Some(rest) = s.strip_prefix(m) {
            return Some((rest, c));
        }
    }
    None
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/er.rs"]
mod tests;
