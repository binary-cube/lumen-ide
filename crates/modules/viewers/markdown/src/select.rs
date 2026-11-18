//! Selecting the preview's text as on a web page: the layout keeps its text as runs (a
//! word, a piece of a code line, a list's marker, a formula, a diagram) in the document's
//! order, each with its place and how it joins the one before when copied. A selection is
//! two offsets among the runs' text (so it lasts while the same document is laid out again:
//! an image arriving, another width); hit-testing looks at the runs' boxes and their shaped
//! glyphs, made by the layout anyway — nothing is worked out while idle.

use std::ops::Range;
use std::sync::Arc;

use kernel::{Point, Rect};
use text_layout::ShapedLine;

/// How a run joins the one before it in the copied text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sep {
    /// Right after (the words of a line, a line wrapped).
    None,
    /// On the next line (a line break, list items, a code block's lines, a table's rows).
    Line,
    /// After an empty line (paragraphs, headings, blocks).
    Paragraph,
    /// After a tab (a table's cells).
    Cell,
}

impl Sep {
    fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Line => "\n",
            Self::Paragraph => "\n\n",
            Self::Cell => "\t",
        }
    }
}

/// A piece of the document's text, where it's shown (document coordinates).
#[derive(Debug, Clone)]
pub struct TextRun {
    /// Its line's box: from where it starts, as wide as it is, as tall as its line.
    pub rect: Rect,
    pub text: String,
    /// Its glyphs (the characters' places); `None`: selected whole (a formula, a diagram,
    /// a list's marker), copied as [`Self::copy`].
    pub shaped: Option<Arc<ShapedLine>>,
    pub copy: String,
    pub sep: Sep,
    /// The block it's in (a paragraph, a heading, a cell, a code line): a triple click
    /// selects it.
    pub block: u32,
    /// The code block that scrolls across it moves it (its place among those).
    pub scroll: Option<usize>,
    /// Where its text starts among all the runs' (offsets).
    pub start: usize,
    /// The part of the source it comes from (its block's; Copy as Markdown).
    pub src: Range<usize>,
}

impl TextRun {
    /// How many offsets it takes: its text's bytes; one for a whole.
    pub fn len(&self) -> usize {
        if self.shaped.is_some() { self.text.len() } else { 1 }
    }

    pub fn end(&self) -> usize {
        self.start + self.len()
    }

    /// The offset at `x` (document coordinates, already moved by its scroll).
    fn offset_at(&self, x: f32) -> usize {
        let dx = x - self.rect.min_x();
        match &self.shaped {
            Some(shaped) => self.start + shaped.byte_at(dx, self.text.len()),
            None => self.start + usize::from(dx > self.rect.size.width / 2.0),
        }
    }

    /// Where offset `at` (inside it) is, across (from its start).
    fn x_of(&self, at: usize) -> f32 {
        let i = at.saturating_sub(self.start);
        match &self.shaped {
            Some(_) if i >= self.text.len() => self.rect.size.width,
            Some(shaped) => shaped.x_at(i),
            None if i == 0 => 0.0,
            None => self.rect.size.width,
        }
    }
}

/// Two offsets: where the selection started and where it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub fn at(offset: usize) -> Self {
        Self { anchor: offset, focus: offset }
    }

    /// From the first offset to the last.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }
}

/// All the runs' offsets.
pub fn total(runs: &[TextRun]) -> usize {
    runs.last().map_or(0, TextRun::end)
}

/// The run holding offset `at` (the one starting there, at a boundary).
fn run_of(runs: &[TextRun], at: usize) -> Option<usize> {
    if runs.is_empty() {
        return None;
    }
    let i = runs.partition_point(|r| r.end() <= at);
    Some(i.min(runs.len() - 1))
}

/// The offset under `p` (document coordinates; `shift`: how far each scrolling code block
/// is scrolled): on a line, its nearest run; between lines, the next line's start; below
/// everything, the end.
pub fn offset_at(runs: &[TextRun], p: Point, shift: &dyn Fn(usize) -> f32) -> usize {
    let x_of = |r: &TextRun| p.x + r.scroll.map_or(0.0, shift);
    let mut best: Option<(f32, usize)> = None;
    for r in runs {
        if p.y < r.rect.min_y() || p.y >= r.rect.max_y() {
            continue;
        }
        let x = x_of(r);
        let (distance, at) = if x < r.rect.min_x() {
            (r.rect.min_x() - x, r.start)
        } else if x > r.rect.max_x() {
            (x - r.rect.max_x(), r.end())
        } else {
            return r.offset_at(x);
        };
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, at));
        }
    }
    if let Some((_, at)) = best {
        return at;
    }
    // Between lines: where the next one down starts.
    let below = runs
        .iter()
        .filter(|r| r.rect.min_y() > p.y)
        .min_by(|a, b| a.rect.min_y().total_cmp(&b.rect.min_y()).then(a.start.cmp(&b.start)));
    below.map_or(total(runs), |r| r.start)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word around offset `at` (a double click): its letters, digits and `_`, across runs
/// that join without a space (`**bold**er`); on anything else, that character; a whole
/// (a formula) alone.
pub fn word_at(runs: &[TextRun], at: usize) -> (usize, usize) {
    let Some(i) = run_of(runs, at) else { return (at, at) };
    let r = &runs[i];
    if r.shaped.is_none() {
        return (r.start, r.end());
    }
    let local = at.saturating_sub(r.start).min(r.text.len());
    let here = r.text[local..].chars().next().or_else(|| r.text[..local].chars().next_back());
    let Some(c) = here else { return (at, at) };
    if !is_word(c) {
        let from = if local < r.text.len() { local } else { local - c.len_utf8() };
        return (r.start + from, r.start + from + c.len_utf8());
    }
    // Back to the word's start, then on to its end, through joined runs.
    let (mut j, mut cut) = (i, local);
    let start = loop {
        let run = &runs[j];
        let kept = run.text[..cut].char_indices().rev().take_while(|(_, c)| is_word(*c)).last();
        let from = kept.map_or(cut, |(k, _)| k);
        let joined = from == 0
            && j > 0
            && run.sep == Sep::None
            && runs[j - 1].shaped.is_some()
            && runs[j - 1].text.chars().next_back().is_some_and(is_word);
        if !joined {
            break run.start + from;
        }
        j -= 1;
        cut = runs[j].text.len();
    };
    let (mut j, mut from) = (i, local);
    let end = loop {
        let run = &runs[j];
        let len: usize =
            run.text[from..].chars().take_while(|c| is_word(*c)).map(char::len_utf8).sum();
        let joined = from + len == run.text.len()
            && j + 1 < runs.len()
            && runs[j + 1].sep == Sep::None
            && runs[j + 1].shaped.is_some()
            && runs[j + 1].text.chars().next().is_some_and(is_word);
        if !joined {
            break run.start + from + len;
        }
        j += 1;
        from = 0;
    };
    (start, end)
}

/// The block around offset `at` (a triple click): its paragraph, heading, cell or line.
pub fn block_at(runs: &[TextRun], at: usize) -> (usize, usize) {
    let Some(i) = run_of(runs, at) else { return (at, at) };
    let block = runs[i].block;
    let first = runs[..i].iter().rposition(|r| r.block != block).map_or(0, |k| k + 1);
    let last = runs[i..].iter().position(|r| r.block != block).map_or(runs.len(), |k| i + k);
    (runs[first].start, runs[last - 1].end())
}

/// The text from offset `a` to `b`, as plain text: lines, paragraphs and cells joined as
/// the runs say, a whole's copy text when it's in.
pub fn text(runs: &[TextRun], a: usize, b: usize) -> String {
    let mut out = String::new();
    let Some(first) = run_of(runs, a) else { return out };
    let mut started = false;
    for r in &runs[first..] {
        if r.start >= b {
            break;
        }
        if r.end() <= a {
            continue;
        }
        if started && r.sep != Sep::None {
            // No spaces left at the end of a line.
            out.truncate(out.trim_end_matches(' ').len());
            out.push_str(r.sep.text());
        }
        started = true;
        if r.shaped.is_none() {
            out.push_str(&r.copy);
            continue;
        }
        let from = a.saturating_sub(r.start).min(r.text.len());
        let to = (b - r.start).min(r.text.len());
        if let Some(piece) = r.text.get(from..to) {
            out.push_str(piece);
        }
    }
    out.truncate(out.trim_end_matches(' ').len());
    out
}

/// The source of the blocks from offset `a` to `b` (whole lines of it): Copy as Markdown.
pub fn markdown(runs: &[TextRun], source: &str, a: usize, b: usize) -> Option<String> {
    let first = run_of(runs, a)?;
    let mut from = usize::MAX;
    let mut to = 0;
    for r in &runs[first..] {
        if r.start >= b {
            break;
        }
        if r.end() <= a || r.src.is_empty() {
            continue;
        }
        from = from.min(r.src.start);
        to = to.max(r.src.end);
    }
    if from >= to || to > source.len() {
        return None;
    }
    // A block's range may take the empty lines after it.
    let to = from + source.get(from..to)?.trim_end().len();
    let from = source[..from].rfind('\n').map_or(0, |i| i + 1);
    let to = source[to..].find('\n').map_or(source.len(), |i| to + i);
    Some(source.get(from..to)?.trim_end().to_owned())
}

/// The boxes of the selection from `a` to `b` over the runs whose lines `visible` says are
/// in view: each box, the code block that scrolls it, and whether it covers a whole.
pub fn boxes(
    runs: &[TextRun],
    a: usize,
    b: usize,
    visible: impl Fn(&Rect) -> bool,
) -> Vec<(Rect, Option<usize>, bool)> {
    let mut out: Vec<(Rect, Option<usize>, bool)> = Vec::new();
    let Some(first) = run_of(runs, a) else { return out };
    for r in &runs[first..] {
        if r.start >= b {
            break;
        }
        if r.end() <= a || !visible(&r.rect) {
            continue;
        }
        let x0 = r.rect.min_x() + r.x_of(a.max(r.start));
        let x1 = r.rect.min_x() + r.x_of(b.min(r.end()));
        if x1 <= x0 {
            continue;
        }
        let rect = Rect::new(x0, r.rect.min_y(), x1 - x0, r.rect.size.height);
        let whole = r.shaped.is_none();
        // The words of a line make one box.
        if let Some(last) = out.last_mut()
            && !whole
            && !last.2
            && last.1 == r.scroll
            && (last.0.min_y() - rect.min_y()).abs() < 0.5
            && (last.0.size.height - rect.size.height).abs() < 0.5
            && (last.0.max_x() - rect.min_x()).abs() < 1.0
        {
            last.0 = Rect::new(
                last.0.min_x(),
                last.0.min_y(),
                rect.max_x() - last.0.min_x(),
                rect.size.height,
            );
            continue;
        }
        out.push((rect, r.scroll, whole));
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/select.rs"]
mod tests;
