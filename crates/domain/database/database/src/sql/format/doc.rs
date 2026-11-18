//! The layout's language (a Wadler / Prettier-style document) and its printer: texts, places
//! that break or not (a space or nothing on one line, a line break otherwise), indented
//! parts and groups — a group is printed on one line when it fits in the width (with what
//! follows it up to the next break), else its places break. A hard break (a line comment's
//! end, a clause's start) breaks every group around it. Printed without recursion (a long
//! statement nests deeply); two breaks in a row never leave a blank line, and no line ends
//! in spaces the printer added.

use std::borrow::Cow;

/// A part of the layout.
#[derive(Debug, Clone)]
pub(super) enum Doc<'a> {
    /// Text as it comes out (a token, a comment: maybe several lines).
    Text(Cow<'a, str>),
    /// A space that never breaks (none at a line's start).
    Space,
    /// A space on one line, a line break otherwise.
    Line,
    /// Nothing on one line, a line break otherwise.
    Soft,
    /// Always a line break.
    Hard,
    /// Its parts, the lines they break onto indented one step more.
    Indent(Box<Part<'a>>),
    /// Its parts on one line if they fit, else broken.
    Group(Box<Part<'a>>),
    /// Its parts, in order.
    Seq(Box<Part<'a>>),
}

/// The parts of an indent, a group or a sequence, and whether a hard break is among them
/// (worked out once, when made).
#[derive(Debug, Clone)]
pub(super) struct Part<'a> {
    docs: Vec<Doc<'a>>,
    hard: bool,
}

impl<'a> Doc<'a> {
    pub(super) fn text(text: impl Into<Cow<'a, str>>) -> Self {
        Doc::Text(text.into())
    }

    pub(super) fn seq(docs: Vec<Doc<'a>>) -> Self {
        Doc::Seq(Box::new(Part::new(docs)))
    }

    pub(super) fn indent(docs: Vec<Doc<'a>>) -> Self {
        Doc::Indent(Box::new(Part::new(docs)))
    }

    pub(super) fn group(docs: Vec<Doc<'a>>) -> Self {
        Doc::Group(Box::new(Part::new(docs)))
    }

    /// A group always broken (one item a line, whatever the width).
    pub(super) fn broken(docs: Vec<Doc<'a>>) -> Self {
        let mut part = Part::new(docs);
        part.hard = true;
        Doc::Group(Box::new(part))
    }

    /// Whether it holds a hard break (the groups around it break).
    pub(super) fn hard(&self) -> bool {
        match self {
            Doc::Text(text) => text.contains(['\n', '\r']),
            Doc::Hard => true,
            Doc::Space | Doc::Line | Doc::Soft => false,
            Doc::Indent(part) | Doc::Group(part) | Doc::Seq(part) => part.hard,
        }
    }
}

impl<'a> Part<'a> {
    fn new(docs: Vec<Doc<'a>>) -> Self {
        let hard = docs.iter().any(Doc::hard);
        Self { docs, hard }
    }
}

/// How lines are printed.
#[derive(Debug, Clone, Copy)]
pub(super) struct Page<'b> {
    /// The width a group must fit in (columns).
    pub width: usize,
    /// One step of indentation (spaces).
    pub indent: usize,
    /// What starts every line after the first (the statement's own indentation).
    pub margin: &'b str,
    /// The column the first line starts at.
    pub column: usize,
    /// How lines end (`\n`, or the text's `\r\n`).
    pub newline: &'b str,
}

/// A text's width in columns (a tab counted as four).
pub(super) fn width(text: &str) -> usize {
    text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

/// `doc` printed on `page`.
pub(super) fn print(doc: &Doc<'_>, page: Page<'_>) -> String {
    let margin = width(page.margin);
    let mut out = Printer {
        out: String::new(),
        column: page.column,
        space: false,
        blank: true,
        line_start: 0,
        page,
        margin,
    };
    let mut stack: Vec<(usize, Mode, &Doc<'_>)> = vec![(0, Mode::Break, doc)];
    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Text(text) => out.text(text),
            Doc::Space => out.space(),
            Doc::Line if mode == Mode::Flat => out.space(),
            Doc::Soft if mode == Mode::Flat => {}
            Doc::Line | Doc::Soft | Doc::Hard => out.newline(indent),
            Doc::Indent(part) => {
                let inner = indent + page.indent;
                stack.extend(part.docs.iter().rev().map(|d| (inner, mode, d)));
            }
            Doc::Seq(part) => stack.extend(part.docs.iter().rev().map(|d| (indent, mode, d))),
            Doc::Group(part) => {
                let flat = mode == Mode::Flat
                    || (!part.hard
                        && fits(
                            &part.docs,
                            &stack,
                            page.width.saturating_sub(out.column),
                            out.space && !out.blank,
                        ));
                let mode = if flat { Mode::Flat } else { Mode::Break };
                stack.extend(part.docs.iter().rev().map(|d| (indent, mode, d)));
            }
        }
    }
    // A break at the end (an empty last item) leaves no empty line.
    if out.blank && out.line_start > 0 {
        out.out.truncate(out.line_start - page.newline.len());
    }
    out.out
}

struct Printer<'b> {
    out: String,
    column: usize,
    /// A space is due before the next text.
    space: bool,
    /// Nothing but indentation on the current line yet.
    blank: bool,
    /// Where the current line's indentation starts in `out`.
    line_start: usize,
    page: Page<'b>,
    margin: usize,
}

impl Printer<'_> {
    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.space && !self.blank {
            self.out.push(' ');
            self.column += 1;
        }
        self.space = false;
        self.blank = false;
        self.out.push_str(text);
        match text.rfind(['\n', '\r']) {
            Some(at) => self.column = width(&text[at + 1..]),
            None => self.column += width(text),
        }
    }

    fn space(&mut self) {
        if !self.blank {
            self.space = true;
        }
    }

    fn newline(&mut self, indent: usize) {
        self.space = false;
        if self.out.is_empty() {
            // The first line starts where the statement is: nothing to break.
            return;
        }
        if self.blank {
            // Two breaks in a row: the second only sets the indentation.
            self.out.truncate(self.line_start);
        } else {
            self.out.push_str(self.page.newline);
            self.line_start = self.out.len();
        }
        self.out.push_str(self.page.margin);
        self.out.extend(std::iter::repeat_n(' ', indent));
        self.column = self.margin + indent;
        self.blank = true;
    }
}

/// Whether `docs`, on one line, and what follows them up to the next break, fit in `room`
/// columns (`space`: a space is due first).
fn fits(docs: &[Doc<'_>], rest: &[(usize, Mode, &Doc<'_>)], room: usize, space: bool) -> bool {
    let mut room = room as isize;
    let mut space = space;
    let mut own: Vec<(Mode, &Doc<'_>)> = docs.iter().rev().map(|d| (Mode::Flat, d)).collect();
    let mut rest = rest.iter().rev();
    loop {
        let (mode, doc) = match own.pop() {
            Some(next) => next,
            None => match rest.next() {
                Some(&(_, mode, doc)) => (mode, doc),
                None => return true,
            },
        };
        match doc {
            Doc::Text(text) => {
                if text.is_empty() {
                    continue;
                }
                if space {
                    room -= 1;
                    space = false;
                }
                match text.find(['\n', '\r']) {
                    Some(at) => return room >= width(&text[..at]) as isize,
                    None => room -= width(text) as isize,
                }
                if room < 0 {
                    return false;
                }
            }
            Doc::Space => space = true,
            Doc::Line if mode == Mode::Flat => space = true,
            Doc::Soft if mode == Mode::Flat => {}
            Doc::Line | Doc::Soft | Doc::Hard => return true,
            Doc::Indent(part) | Doc::Seq(part) => {
                own.extend(part.docs.iter().rev().map(|d| (mode, d)));
            }
            Doc::Group(part) => {
                let mode = if part.hard { Mode::Break } else { mode };
                own.extend(part.docs.iter().rev().map(|d| (mode, d)));
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/sql/format/doc.rs"]
mod tests;
