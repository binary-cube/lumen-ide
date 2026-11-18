//! A conversation's items as lines to draw at a width: each message wrapped by paragraph,
//! each tool a line (its status, its title) and a line for each file it changes, the plan's
//! steps. Kept per item by the change that last touched it ([`Laid::revision`], the
//! conversation's stamp, never its content hashed): a new chunk lays out only its message
//! again, a wider tab everything (cheaply: a diff's counts came with it). A message that
//! grows (it only ever does, chunk by chunk) keeps the lines of its finished paragraphs and
//! wraps again only its last one.

use acp::{Author, Item, ToolStatus};
use text_layout::{Font, TextSystem};
use ui_components::ds;

/// How a line is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Text,
    Muted,
    Accent,
    Danger,
}

/// What a click on a line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Diff `diff` of the tool that's item `item`, compared.
    Diff { item: usize, diff: usize },
}

/// A line to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub ink: Ink,
    /// Before the text (logical pixels).
    pub indent: f32,
    pub icon: Option<&'static str>,
    pub link: Option<Link>,
}

impl Line {
    fn plain(text: String, ink: Ink) -> Self {
        Self { text, ink, indent: 0.0, icon: None, link: None }
    }
}

/// An item laid out: the item's revision it was laid out for ([`acp::Conversation::revision`]:
/// laid out again when it moves), its lines, whether it's the user's (drawn on a bubble), and
/// for a message what of it is finished.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Laid {
    pub revision: u64,
    pub lines: Vec<Line>,
    pub bubble: bool,
    settled: Option<Settled>,
}

/// A message's finished paragraphs (all but its last): the same however it grows, since a
/// chunk only adds to its end.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Settled {
    author: Author,
    /// Where its last paragraph starts (bytes, after a `\n`; 0: it has one).
    at: usize,
    /// The lines before it.
    lines: usize,
    /// How long the message was.
    len: usize,
}

/// `value` in lines no wider than `width`, paragraph by paragraph (an empty one kept), and
/// where its last paragraph starts with the lines before it (`Settled::at`, `Settled::lines`).
fn wrapped(
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    width: f32,
    ink: Ink,
) -> (Vec<Line>, usize, usize) {
    let trimmed = value.trim_end();
    let last = trimmed.rfind('\n').map_or(0, |i| i + 1);
    let mut lines = Vec::new();
    let mut before = 0;
    let mut offset = 0;
    for paragraph in trimmed.split('\n') {
        if offset == last {
            before = lines.len();
        }
        offset += paragraph.len() + 1;
        if paragraph.trim().is_empty() {
            lines.push(Line::plain(String::new(), ink));
            continue;
        }
        for line in ds::wrap(text, paragraph, font, width.max(1.0), usize::MAX) {
            lines.push(Line::plain(line, ink));
        }
    }
    (lines, last, before)
}

/// A message's ink, its bubble, its room at `width`.
fn message_look(author: Author, width: f32) -> (Ink, bool, f32) {
    let ink = if author == Author::Thought { Ink::Muted } else { Ink::Text };
    let bubble = author == Author::User;
    // A bubble's text keeps its padding inside the width.
    let room = if bubble { width * 0.8 } else { width };
    (ink, bubble, room)
}

/// Item `index` laid out again at the same `width` as `old` was: a message that grew wraps
/// only its last paragraph again (its finished ones kept), anything else anew.
pub fn lay_on(
    old: Laid,
    item: &Item,
    (index, revision): (usize, u64),
    text: &mut TextSystem,
    font: &Font,
    width: f32,
) -> Laid {
    let Item::Message { author, text: said } = item else {
        return lay(item, (index, revision), text, font, width);
    };
    let grown = old.settled.filter(|s| {
        s.author == *author
            && said.len() >= s.len
            && said.is_char_boundary(s.at)
            && (s.at == 0 || said.as_bytes()[s.at - 1] == b'\n')
    });
    let Some(settled) = grown else { return lay(item, (index, revision), text, font, width) };
    let (ink, bubble, room) = message_look(*author, width);
    let mut lines = old.lines;
    lines.truncate(settled.lines);
    let (tail, at, before) = wrapped(text, &said[settled.at..], font, room, ink);
    lines.extend(tail);
    let settled = Settled {
        author: *author,
        at: settled.at + at,
        lines: settled.lines + before,
        len: said.len(),
    };
    Laid { revision, lines, bubble, settled: Some(settled) }
}

/// The file's name and folder as shown (`a.php — src/Auth`).
fn short_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    match path.rsplit_once('/') {
        Some((dir, name)) => {
            let dir =
                dir.rsplit('/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>();
            format!("{name} — {}", dir.join("/"))
        }
        None => path,
    }
}

/// Item `index`'s lines at `width` (physical pixels), as of its `revision`.
pub fn lay(
    item: &Item,
    (index, revision): (usize, u64),
    text: &mut TextSystem,
    font: &Font,
    width: f32,
) -> Laid {
    let k = revision;
    match item {
        Item::Message { author, text: said } => {
            let (ink, bubble, room) = message_look(*author, width);
            let (lines, at, before) = wrapped(text, said, font, room, ink);
            let settled = Settled { author: *author, at, lines: before, len: said.len() };
            Laid { revision: k, lines, bubble, settled: Some(settled) }
        }
        Item::Tool(tool) => {
            let (icon, ink) = match tool.status {
                ToolStatus::Pending => ("circle-help", Ink::Muted),
                ToolStatus::Running => ("play", Ink::Accent),
                ToolStatus::Done => ("check", Ink::Muted),
                ToolStatus::Failed => ("x", Ink::Danger),
            };
            let title = if tool.title.is_empty() { tool.kind.clone() } else { tool.title.clone() };
            let title = ds::ellipsize(text, &title, font, width - 24.0);
            let mut lines =
                vec![Line { text: title, ink, indent: 0.0, icon: Some(icon), link: None }];
            for (i, change) in tool.diffs.iter().enumerate() {
                let label =
                    format!("{}  +{} −{}", short_path(&change.path), change.added, change.removed);
                let label = ds::ellipsize(text, &label, font, width - 48.0);
                lines.push(Line {
                    text: label,
                    ink: Ink::Accent,
                    indent: 24.0,
                    icon: Some("arrow-up-right"),
                    link: Some(Link::Diff { item: index, diff: i }),
                });
            }
            Laid { revision: k, lines, bubble: false, settled: None }
        }
        Item::Plan(steps) => {
            let lines = steps
                .iter()
                .map(|s| Line {
                    text: ds::ellipsize(text, &s.text, font, width - 24.0),
                    ink: if s.done {
                        Ink::Muted
                    } else if s.running {
                        Ink::Accent
                    } else {
                        Ink::Text
                    },
                    indent: 0.0,
                    icon: Some(if s.done { "check" } else { "square" }),
                    link: None,
                })
                .collect();
            Laid { revision: k, lines, bubble: false, settled: None }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
