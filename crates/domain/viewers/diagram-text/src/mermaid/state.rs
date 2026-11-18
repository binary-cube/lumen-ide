//! `stateDiagram` / `stateDiagram-v2`: states (named, described, `state "…" as id`),
//! the start and end (`[*]`, one of each per composite state), transitions with labels,
//! composite states (nested, with their concurrent regions split by `--`), forks, joins,
//! choices, notes (one line or until `end note`), `direction` and styles.

use std::collections::HashMap;

use super::{Direction, Error, ErrorKind, Line, Style, label};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateDiagram {
    pub direction: Direction,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    #[default]
    Normal,
    Start,
    End,
    Fork,
    Join,
    Choice,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub id: String,
    pub label: String,
    pub descriptions: Vec<String>,
    pub kind: Kind,
    /// The composite state it's in, and which of its regions (0 when it has one).
    pub parent: Option<usize>,
    pub region: usize,
    /// It holds others (drawn as a frame around them).
    pub composite: bool,
    /// How many regions it has (a composite's `--`).
    pub regions: usize,
    pub style: Style,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transition {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    pub state: usize,
    pub left: bool,
    pub text: String,
}

#[derive(Default)]
struct Parser {
    d: StateDiagram,
    ids: HashMap<String, usize>,
    /// The composite states open (innermost last), with the line each opened on.
    open: Vec<(usize, usize)>,
    classes: HashMap<String, Style>,
    given: Vec<(usize, String)>,
    /// A note being read until `end note`.
    note: Option<Note>,
}

pub(super) fn parse(lines: &[Line<'_>]) -> Result<StateDiagram, Error> {
    let mut p = Parser::default();
    for line in lines {
        let t = line.text.trim();
        if let Some(note) = &mut p.note {
            if t.eq_ignore_ascii_case("end note") {
                let note = p.note.take();
                p.d.notes.extend(note);
            } else {
                if !note.text.is_empty() {
                    note.text.push('\n');
                }
                note.text.push_str(t);
            }
            continue;
        }
        if t.is_empty() {
            continue;
        }
        p.statement(line)?;
    }
    if let Some(&(_, number)) = p.open.last() {
        return Err(Error::new(number, ErrorKind::Unclosed("}")));
    }
    for (s, name) in &p.given {
        if let Some(style) = p.classes.get(name) {
            let state = &mut p.d.states[*s];
            state.style = state.style.merged(style);
        }
    }
    Ok(p.d)
}

impl Parser {
    fn scope(&self) -> (Option<usize>, usize) {
        match self.open.last() {
            Some(&(c, _)) => (Some(c), self.d.states[c].regions.saturating_sub(1)),
            None => (None, 0),
        }
    }

    /// The state named `id` (`[*]`: the scope's start or end), made if it's new.
    fn state(&mut self, id: &str, as_target: bool) -> usize {
        let id = id.trim();
        let (id, class) = match id.split_once(":::") {
            Some((i, c)) => (i.trim(), Some(c.trim().to_owned())),
            None => (id, None),
        };
        let (parent, region) = self.scope();
        let (key, kind) = if id == "[*]" {
            let kind = if as_target { Kind::End } else { Kind::Start };
            (format!("[*]{kind:?}{parent:?}{region}"), kind)
        } else {
            (id.to_owned(), Kind::Normal)
        };
        let s = match self.ids.get(&key) {
            Some(&s) => s,
            None => {
                let s = self.d.states.len();
                let label = if kind == Kind::Normal { label(id) } else { String::new() };
                self.d.states.push(State {
                    id: key.clone(),
                    label,
                    kind,
                    parent,
                    region,
                    ..State::default()
                });
                self.ids.insert(key, s);
                s
            }
        };
        if let Some(c) = class {
            self.given.push((s, c));
        }
        s
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
            "}" => {
                return match self.open.pop() {
                    Some(_) => Ok(()),
                    None => Err(Error::unexpected(line)),
                };
            }
            "--" => {
                let Some(&(c, _)) = self.open.last() else { return Err(Error::unexpected(line)) };
                self.d.states[c].regions += 1;
                return Ok(());
            }
            "state" => return self.state_statement(rest, line),
            "note" => return self.note_statement(rest, line),
            "classDef" => {
                let (names, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                for name in names.split(',') {
                    self.classes.entry(name.trim().to_owned()).or_default().apply(style);
                }
                return Ok(());
            }
            "class" => {
                let (ids, name) =
                    rest.rsplit_once(char::is_whitespace).ok_or_else(|| Error::unexpected(line))?;
                for id in ids.split(',') {
                    let s = self.state(id, false);
                    self.given.push((s, name.trim().to_owned()));
                }
                return Ok(());
            }
            "style" => {
                let (id, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let s = self.state(id, false);
                let style = Style::parse(style);
                let state = &mut self.d.states[s];
                state.style = state.style.merged(&style);
                return Ok(());
            }
            "hide" | "scale" => return Ok(()),
            _ => {}
        }
        // `A --> B : label`.
        if let Some(arrow) = t.find("-->") {
            let from = t[..arrow].trim();
            let after = &t[arrow + 3..];
            let (to, text) = match after.split_once(':') {
                Some((to, text)) => (to.trim(), Some(label(text.trim()))),
                None => (after.trim(), None),
            };
            if from.is_empty() || to.is_empty() {
                return Err(Error::unexpected(line));
            }
            let from = self.state(from, false);
            let to = self.state(to, true);
            self.d.transitions.push(Transition { from, to, label: text.filter(|t| !t.is_empty()) });
            return Ok(());
        }
        // `A : description`.
        if let Some((id, text)) = t.split_once(':') {
            let id = id.trim();
            if id.is_empty() || id.contains(char::is_whitespace) {
                return Err(Error::unexpected(line));
            }
            let s = self.state(id, false);
            self.d.states[s].descriptions.push(label(text.trim()));
            return Ok(());
        }
        if t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ':') {
            self.state(t, false);
            return Ok(());
        }
        Err(Error::unexpected(line))
    }

    /// `state "Label" as id`, `state id <<fork>>`, `state id {`.
    fn state_statement(&mut self, rest: &str, line: &Line<'_>) -> Result<(), Error> {
        let (rest, open) = match rest.strip_suffix('{') {
            Some(r) => (r.trim(), true),
            None => (rest, false),
        };
        let (rest, kind) = match rest.find("<<") {
            Some(i) => {
                let k = rest[i + 2..].trim_end_matches(">>").trim().to_ascii_lowercase();
                let kind = match k.as_str() {
                    "fork" => Kind::Fork,
                    "join" => Kind::Join,
                    "choice" => Kind::Choice,
                    _ => Kind::Normal,
                };
                (rest[..i].trim(), kind)
            }
            None => (rest, Kind::Normal),
        };
        let (id, shown) = if rest.starts_with('"') {
            let (text, id) = rest.rsplit_once(" as ").ok_or_else(|| Error::unexpected(line))?;
            (id.trim(), Some(label(text.trim())))
        } else if let Some((id, text)) = rest.split_once(" as ") {
            // `state id as "Label"` (rare).
            (id.trim(), Some(label(text.trim())))
        } else {
            (rest, None)
        };
        if id.is_empty() {
            return Err(Error::unexpected(line));
        }
        let s = self.state(id, false);
        let state = &mut self.d.states[s];
        if let Some(shown) = shown {
            state.label = shown;
        }
        if kind != Kind::Normal {
            state.kind = kind;
        }
        if open {
            state.composite = true;
            state.regions = state.regions.max(1);
            self.open.push((s, line.number));
        }
        Ok(())
    }

    /// `note right of A : text`, or `note left of A` then lines until `end note`.
    fn note_statement(&mut self, rest: &str, line: &Line<'_>) -> Result<(), Error> {
        let (left, after) = if let Some(r) = rest.strip_prefix("left of") {
            (true, r)
        } else if let Some(r) = rest.strip_prefix("right of") {
            (false, r)
        } else {
            return Err(Error::unexpected(line));
        };
        let (id, text) = match after.split_once(':') {
            Some((id, text)) => (id.trim(), Some(label(text.trim()))),
            None => (after.trim(), None),
        };
        let state = self.state(id, false);
        match text {
            Some(text) => self.d.notes.push(Note { state, left, text }),
            None => self.note = Some(Note { state, left, text: String::new() }),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/state.rs"]
mod tests;
