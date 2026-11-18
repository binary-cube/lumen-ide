//! `sequenceDiagram`: participants and actors (with aliases, in boxes), messages (their
//! lines and heads, activations), notes, activations, blocks (`loop`, `alt` / `else`,
//! `opt`, `par` / `and`, `critical` / `option`, `break`, `rect`) and `autonumber`.

use kernel::Rgba;

use super::{Error, ErrorKind, Line, Meta, color, label};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sequence {
    pub participants: Vec<Participant>,
    pub items: Vec<Item>,
    pub boxes: Vec<Group>,
    /// Messages numbered (from 1).
    pub autonumber: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub label: String,
    /// Drawn as a person.
    pub actor: bool,
}

/// `box Aqua Title … end`: participants drawn on a background.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Group {
    pub title: String,
    pub color: Option<Rgba>,
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tip {
    None,
    /// `>>`
    Filled,
    /// `x`
    Cross,
    /// `)`: asynchronous.
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    /// `rect rgb(…)`: only a background.
    Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    LeftOf(usize),
    RightOf(usize),
    Over(usize, usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Message {
        from: usize,
        to: usize,
        text: String,
        dotted: bool,
        tip: Tip,
        /// `<<->>`: a tip at both ends.
        both: bool,
        /// `+` before the target: it's activated; `-` before it: the source is
        /// deactivated.
        activate: Option<bool>,
        line: usize,
    },
    Note {
        at: Placement,
        text: String,
    },
    Activate(usize),
    Deactivate(usize),
    Start {
        kind: BlockKind,
        label: String,
        color: Option<Rgba>,
    },
    /// `else`, `and`, `option`: the block's next part.
    Part {
        label: String,
    },
    End,
}

/// The arrows, longest first (a shorter one is a prefix of a longer).
const ARROWS: &[(&str, bool, Tip, bool)] = &[
    ("<<-->>", true, Tip::Filled, true),
    ("<<->>", false, Tip::Filled, true),
    ("-->>", true, Tip::Filled, false),
    ("->>", false, Tip::Filled, false),
    ("--x", true, Tip::Cross, false),
    ("-x", false, Tip::Cross, false),
    ("--)", true, Tip::Open, false),
    ("-)", false, Tip::Open, false),
    ("-->", true, Tip::None, false),
    ("->", false, Tip::None, false),
];

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Sequence, Error> {
    let mut seq = Sequence::default();
    let mut depth: Vec<(BlockKind, usize)> = Vec::new();
    let mut open_box: Option<(usize, usize)> = None;
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "participant" | "actor" => {
                let (id, alias) = match rest.split_once(" as ") {
                    Some((id, alias)) => (id.trim(), Some(label(alias.trim()))),
                    None => (rest, None),
                };
                let p = participant(&mut seq, id);
                seq.participants[p].actor = word == "actor";
                if let Some(alias) = alias {
                    seq.participants[p].label = alias;
                }
                if let Some((b, _)) = open_box {
                    seq.boxes[b].members.push(p);
                }
            }
            "create" => {}
            "destroy" => {}
            "autonumber" => seq.autonumber = true,
            "title" => meta.title = Some(rest.trim_start_matches(':').trim().to_owned()),
            "activate" => {
                let p = participant(&mut seq, rest);
                seq.items.push(Item::Activate(p));
            }
            "deactivate" => {
                let p = participant(&mut seq, rest);
                seq.items.push(Item::Deactivate(p));
            }
            "box" => {
                let (color, title) = box_head(rest);
                seq.boxes.push(Group { title, color, members: Vec::new() });
                open_box = Some((seq.boxes.len() - 1, line.number));
            }
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" => {
                let kind = match word {
                    "loop" => BlockKind::Loop,
                    "alt" => BlockKind::Alt,
                    "opt" => BlockKind::Opt,
                    "par" => BlockKind::Par,
                    "critical" => BlockKind::Critical,
                    "break" => BlockKind::Break,
                    _ => BlockKind::Rect,
                };
                let color = (kind == BlockKind::Rect).then(|| color(rest)).flatten();
                let label = if kind == BlockKind::Rect { String::new() } else { label(rest) };
                seq.items.push(Item::Start { kind, label, color });
                depth.push((kind, line.number));
            }
            "else" | "and" | "option" => {
                if depth.is_empty() {
                    return Err(Error::unexpected(line));
                }
                seq.items.push(Item::Part { label: label(rest) });
            }
            "end" => {
                if depth.pop().is_some() {
                    seq.items.push(Item::End);
                } else if open_box.take().is_none() {
                    return Err(Error::unexpected(line));
                }
            }
            w if w.eq_ignore_ascii_case("note") => {
                let item = note(&mut seq, rest).ok_or_else(|| Error::unexpected(line))?;
                seq.items.push(item);
            }
            _ => {
                let item =
                    message(&mut seq, t, line.number).ok_or_else(|| Error::unexpected(line))?;
                seq.items.push(item);
            }
        }
    }
    if let Some((_, number)) = depth.last() {
        return Err(Error::new(*number, ErrorKind::Unclosed("end")));
    }
    if let Some((_, number)) = open_box {
        return Err(Error::new(number, ErrorKind::Unclosed("end")));
    }
    Ok(seq)
}

/// The participant named `id`, added if it's new.
fn participant(seq: &mut Sequence, id: &str) -> usize {
    let id = id.trim();
    if let Some(p) = seq.participants.iter().position(|p| p.id == id) {
        return p;
    }
    seq.participants.push(Participant { id: id.to_owned(), label: label(id), actor: false });
    seq.participants.len() - 1
}

/// A box's color (if its first word is one) and title.
fn box_head(rest: &str) -> (Option<Rgba>, String) {
    if rest.starts_with("rgb")
        && let Some(end) = rest.find(')')
    {
        return (color(&rest[..=end]), label(rest[end + 1..].trim()));
    }
    let first = rest.split_whitespace().next().unwrap_or_default();
    match color(first) {
        Some(c) => (Some(c), label(rest[first.len()..].trim())),
        None => (None, label(rest)),
    }
}

/// `right of A: text`, `left of A: text`, `over A,B: text`.
fn note(seq: &mut Sequence, rest: &str) -> Option<Item> {
    let (place, text) = rest.split_once(':')?;
    let place = place.trim();
    let at = if let Some(p) = place.strip_prefix("right of") {
        Placement::RightOf(participant(seq, p))
    } else if let Some(p) = place.strip_prefix("left of") {
        Placement::LeftOf(participant(seq, p))
    } else {
        let p = place.strip_prefix("over")?;
        let mut names = p.split(',');
        let a = participant(seq, names.next()?);
        let b = names.next().map_or(a, |n| participant(seq, n));
        Placement::Over(a, b)
    };
    Some(Item::Note { at, text: label(text.trim()) })
}

/// `A->>+B: text`.
fn message(seq: &mut Sequence, t: &str, line: usize) -> Option<Item> {
    let (head, text) = match t.split_once(':') {
        Some((head, text)) => (head, label(text.trim())),
        None => (t, String::new()),
    };
    let (at, (arrow, dotted, tip, both)) = (1..head.len())
        .filter(|&i| head.is_char_boundary(i))
        .find_map(|i| ARROWS.iter().find(|a| head[i..].starts_with(a.0)).map(|a| (i, *a)))?;
    let from = head[..at].trim();
    let mut to = head[at + arrow.len()..].trim();
    let activate = match to.chars().next() {
        Some('+') => Some(true),
        Some('-') => Some(false),
        _ => None,
    };
    if activate.is_some() {
        to = to[1..].trim();
    }
    if from.is_empty() || to.is_empty() {
        return None;
    }
    let from = participant(seq, from);
    let to = participant(seq, to);
    Some(Item::Message { from, to, text, dotted, tip, both, activate, line })
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/sequence.rs"]
mod tests;
