//! The walk over the parser's events: what the open blocks and the current inline style
//! hold, and how each Markdown (and HTML) element is laid out with the [`Builder`].
//!
//! GitHub's flavor on top of CommonMark: tables (aligned), task lists (checkable), alerts
//! (`> [!NOTE]`, and Obsidian's callouts, folding), footnotes (numbered, gathered at the
//! end), bare URLs as links, `:emoji:`, heading anchors and a table of contents (`[TOC]`,
//! `[[_TOC_]]`), front matter as a table, definition lists, `==marks==`, wikilinks and the
//! HTML READMEs use (`<details>`, `<kbd>`, `<sup>`, `<img width>`, `<p align>`).

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use kernel::{Rect, Rgba};
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};
use text_layout::{Font, TextSystem};
use ui_components::Palette;

use crate::blocks::{self, TableState};
use crate::html::{self, Token};
use crate::inline::{self, Piece};
use crate::layout::{
    Align, Builder, Caches, Hit, InlineImage, Item, Layout, Page, Run, Span, Style,
};
use crate::select::Sep;

mod extras;
mod formulas;
mod html_tags;
mod lists;

/// The parser's extensions the preview reads.
pub(crate) fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_MATH
}

/// An alert's (callout's) kind: its color and icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    /// Obsidian's quote-like ones (`[!quote]`, `[!example]`).
    Plain,
}

impl Alert {
    fn from_gfm(kind: BlockQuoteKind) -> Self {
        match kind {
            BlockQuoteKind::Note => Self::Note,
            BlockQuoteKind::Tip => Self::Tip,
            BlockQuoteKind::Important => Self::Important,
            BlockQuoteKind::Warning => Self::Warning,
            BlockQuoteKind::Caution => Self::Caution,
        }
    }

    /// An Obsidian callout's type (`[!info]`), as the closest of GitHub's.
    fn from_callout(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "tip" | "hint" | "success" | "check" | "done" => Self::Tip,
            "important" => Self::Important,
            "warning" | "attention" | "question" | "help" | "faq" => Self::Warning,
            "caution" | "danger" | "error" | "bug" | "failure" | "fail" | "missing" => {
                Self::Caution
            }
            "quote" | "cite" | "example" => Self::Plain,
            _ => Self::Note,
        }
    }

    fn color(self, p: &Palette) -> Rgba {
        let ansi = &p.terminal_ansi;
        match self {
            Self::Note => ansi[4],
            Self::Tip => ansi[2],
            Self::Important => ansi[5],
            Self::Warning => ansi[3],
            Self::Caution => ansi[1],
            Self::Plain => p.text_muted,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Note => "info",
            Self::Tip => "lightbulb",
            Self::Important => "comment",
            Self::Warning => "alert",
            Self::Caution => "octagon-alert",
            Self::Plain => "comment",
        }
    }

    fn title(self) -> String {
        i18n::t(match self {
            Self::Note => "markdown.alert_note",
            Self::Tip => "markdown.alert_tip",
            Self::Important => "markdown.alert_important",
            Self::Warning => "markdown.alert_warning",
            Self::Caution => "markdown.alert_caution",
            Self::Plain => "markdown.alert_quote",
        })
    }
}

struct ListState {
    /// Next number (numbered lists) or `None` (bulleted).
    next: Option<u64>,
}

/// What stands before a list item's text.
enum Marker {
    Text(String),
    /// A task's checkbox: done, and the source's `[ ]` it toggles.
    Task(bool, Range<usize>),
}

struct Quote {
    start: f32,
    alert: Option<Alert>,
}

/// What's being left out: a folded block's events (until its end), or a closed
/// `<details>`' (until its `</details>`).
enum Skip {
    Block { depth: usize },
    Details { nesting: usize },
}

/// A heading as the first pass found it: its level, its text and its anchor.
struct Heading {
    level: u8,
    text: String,
    anchor: String,
}

/// What a first pass over the document finds: its headings (the table of contents, the
/// anchors) and the footnotes' numbers, in the order they're referenced.
#[derive(Default)]
struct Outline {
    headings: Vec<Heading>,
    footnotes: HashMap<String, usize>,
}

impl Outline {
    fn of(source: &str) -> Self {
        let mut outline = Self::default();
        let mut seen = HashMap::new();
        let mut heading: Option<(u8, Option<String>, String)> = None;
        let mut definitions = Vec::new();
        for event in Parser::new_ext(source, options()) {
            match event {
                Event::Start(Tag::Heading { level, id, .. }) => {
                    heading = Some((level_number(level), id.map(|i| i.to_string()), String::new()));
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some((_, _, text)) = &mut heading {
                        text.push_str(&t);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((level, id, text)) = heading.take() {
                        let text = inline::emojify(&text).into_owned();
                        let anchor = match id {
                            Some(id) => id,
                            None => inline::slug(&text, &mut seen),
                        };
                        outline.headings.push(Heading { level, text, anchor });
                    }
                }
                Event::FootnoteReference(label) => {
                    let n = outline.footnotes.len() + 1;
                    outline.footnotes.entry(label.to_string()).or_insert(n);
                }
                Event::Start(Tag::FootnoteDefinition(label)) => definitions.push(label.to_string()),
                _ => {}
            }
        }
        // The ones never referenced come after, in the order they're defined.
        for label in definitions {
            let n = outline.footnotes.len() + 1;
            outline.footnotes.entry(label).or_insert(n);
        }
        outline
    }
}

fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// A footnote's definition, kept to be laid out at the end: its label and its events.
type Definition = (String, Vec<(Event<'static>, Range<usize>)>);

/// Current context of a text block.
#[derive(Clone, Copy, PartialEq)]
enum Block {
    Paragraph,
    Heading(u8),
}

pub(crate) struct Walk<'a> {
    pub b: Builder<'a>,
    pub caches: &'a mut Caches,
    dir: &'a Path,
    pub source: &'a str,
    toggled: &'a HashSet<usize>,
    spans: Vec<Span>,
    style: Style,
    link: Option<String>,
    block: Block,
    lists: Vec<ListState>,
    item_marker: Option<Marker>,
    quotes: Vec<Quote>,
    /// A code block being read: its language, its text, where it starts in the source.
    code: Option<(String, String, usize)>,
    image: Option<(String, String)>,
    table: Option<TableState>,
    /// The alignment HTML asked for (`<p align="center">`), innermost last.
    align: Vec<Align>,
    outline: Outline,
    /// The next heading's index in the outline.
    heading: usize,
    /// The footnotes' definitions gathered (laid out at the end), and the one being read
    /// (its label, its events, how deep inside it the walk is).
    definitions: Vec<Definition>,
    reading: Option<(Definition, usize)>,
    skip: Option<Skip>,
    /// The folds met so far (`<details>`, folding callouts): the next one's index.
    folds: usize,
    /// The `<details>` open (whether each is shown open).
    details: Vec<bool>,
    /// A `<summary>` being read: its fold's index and whether it's open.
    summary: Option<(usize, bool)>,
    /// An HTML block's text, read whole before it's laid out.
    html_block: Option<String>,
    /// A callout's first line (its `[!type] title`) is left out of its text.
    callout_line: bool,
    /// Definition lists' definitions (indented).
    definitions_depth: usize,
    /// Laying out the footnotes (smaller text).
    small: bool,
    /// A front matter's text being read.
    metadata: Option<String>,
    /// How many diagrams were met.
    diagrams: usize,
    /// The document has formulas (the math font stays loaded).
    uses_math: bool,
}

impl Layout {
    pub fn build(
        page: Page<'_>,
        caches: &mut Caches,
        text: &mut TextSystem,
        palette: &Palette,
    ) -> Self {
        let Page { source, version, width, scale, dir, toggled } = page;
        caches.begin();
        let s = scale;
        let padding = (32.0 * s).round();
        let reading = (820.0 * s).round();
        let content = (width - 2.0 * padding).clamp(40.0 * s, reading);
        let left = ((width - content) / 2.0).round();
        let b = Builder {
            text,
            palette,
            s,
            left,
            right: left + content,
            y: (28.0 * s).round(),
            items: Vec::new(),
            hits: Vec::new(),
            anchors: Vec::new(),
            code_blocks: Vec::new(),
            diagrams: Vec::new(),
            scrolls: 0,
            body: ((palette.ui_font_size + 2.0) * s).round(),
            runs: Vec::new(),
            sep: None,
            blocks: 0,
            src: 0..0,
        };
        let mut w = Walk {
            b,
            caches,
            dir,
            source,
            toggled,
            spans: Vec::new(),
            style: Style::default(),
            link: None,
            block: Block::Paragraph,
            lists: Vec::new(),
            item_marker: None,
            quotes: Vec::new(),
            code: None,
            image: None,
            table: None,
            align: Vec::new(),
            outline: Outline::of(source),
            heading: 0,
            definitions: Vec::new(),
            reading: None,
            skip: None,
            folds: 0,
            details: Vec::new(),
            summary: None,
            html_block: None,
            callout_line: false,
            definitions_depth: 0,
            small: false,
            metadata: None,
            diagrams: 0,
            uses_math: false,
        };
        for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
            w.event(event, range);
        }
        w.flush_paragraph(false);
        w.footnotes();
        let Walk { b, caches, uses_math, .. } = w;
        // No formula left: the math font goes (once no other preview needs it).
        if !uses_math {
            caches.math_text = None;
        }
        let height = b.y + b.px(48.0);
        caches.end();
        Self {
            version,
            wrap: crate::settings::wrap_code(),
            width,
            scale,
            height,
            items: b.items,
            hits: b.hits,
            anchors: b.anchors,
            code_blocks: b.code_blocks,
            diagrams: b.diagrams,
            lines: crate::sync::LineMap::build(&b.runs, source, height),
            runs: b.runs,
        }
    }
}

impl Walk<'_> {
    pub fn gap(&self) -> f32 {
        (self.b.body * 0.75).round()
    }

    fn body_size(&self) -> f32 {
        if self.small { (self.b.body * 0.88).round() } else { self.b.body }
    }

    /// Left edge of the text (lists, quotes and definitions push it to the right).
    fn indent(&self) -> f32 {
        self.b.left
            + self.lists.len() as f32 * self.b.px(24.0)
            + self.quotes.len() as f32 * self.b.px(18.0)
            + self.definitions_depth as f32 * self.b.px(24.0)
    }

    fn align(&self) -> Align {
        self.align.last().copied().unwrap_or_default()
    }

    /// The color of body text here (a plain quote's is muted).
    fn text_color(&self) -> Rgba {
        let palette = self.b.palette;
        match self.quotes.last() {
            Some(Quote { alert: None, .. }) => palette.text_muted,
            _ => palette.text,
        }
    }

    /// Lays out the pending spans as body text from `x0` to the right margin.
    fn flow_body(&mut self, x0: f32, color: Rgba) {
        let flowed = std::mem::take(&mut self.spans);
        let (size, right, align) = (self.body_size(), self.b.right, self.align());
        self.b.flow(
            &flowed,
            Run { x0, x1: right, size, weight: Font::NORMAL, color, line: 1.6, align },
        );
    }

    /// A span of text in `style`, inside the current link.
    fn push(&mut self, text: &str, style: Style) {
        if self.callout_line || text.is_empty() {
            return;
        }
        self.spans.push(Span::text(text, style, self.link.clone()));
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        // A footnote's definition is kept for the end.
        if let Some(((_, events), depth)) = &mut self.reading {
            match &event {
                Event::Start(_) => *depth += 1,
                Event::End(TagEnd::FootnoteDefinition) if *depth == 0 => {
                    if let Some((definition, _)) = self.reading.take() {
                        self.definitions.push(definition);
                    }
                    return;
                }
                Event::End(_) => *depth -= 1,
                _ => {}
            }
            events.push((event.into_static(), range));
            return;
        }
        match &mut self.skip {
            Some(Skip::Block { depth }) => match &event {
                Event::Start(_) => {
                    *depth += 1;
                    return;
                }
                Event::End(_) if *depth > 0 => {
                    *depth -= 1;
                    return;
                }
                Event::End(_) => self.skip = None,
                _ => return,
            },
            Some(Skip::Details { .. })
                if !matches!(
                    event,
                    Event::Start(Tag::HtmlBlock)
                        | Event::End(TagEnd::HtmlBlock)
                        | Event::Html(_)
                        | Event::InlineHtml(_)
                ) =>
            {
                return;
            }
            Some(Skip::Details { .. }) | None => {}
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(end) => self.end(end),
            Event::TaskListMarker(done) => self.item_marker = Some(Marker::Task(done, range)),
            Event::Rule => self.rule(),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => self.push(&t, Style { code: true, ..self.style }),
            Event::SoftBreak | Event::HardBreak if self.callout_line => self.callout_line = false,
            Event::SoftBreak => self.push(" ", self.style),
            Event::HardBreak => self.push("\n", self.style),
            Event::FootnoteReference(label) => self.footnote_reference(&label),
            Event::Html(html) => match &mut self.html_block {
                Some(block) => block.push_str(&html),
                None => self.html(&html),
            },
            Event::InlineHtml(html) => self.html(&html),
            Event::InlineMath(t) => self.inline_math(&t),
            Event::DisplayMath(t) => self.display_math(&t),
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        // The source of the block the text that follows is in (Copy as Markdown); set once
        // the text before (an item's, laid out when the next starts) is out.
        let block = matches!(
            tag,
            Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::Item
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::Table(_)
                | Tag::HtmlBlock
                | Tag::FootnoteDefinition(_)
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
                | Tag::MetadataBlock(_)
        );
        self.start_tag(tag, range.clone());
        if block {
            self.b.src = range;
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Heading { level, .. } => self.heading_start(level),
            Tag::List(start) => {
                self.flush_item();
                self.lists.push(ListState { next: start });
            }
            Tag::Item => self.item_start(),
            Tag::BlockQuote(kind) => self.quote_start(kind, range),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => {
                        l.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.flush_paragraph(false);
                self.code = Some((lang, String::new(), range.start));
            }
            Tag::HtmlBlock => self.html_block = Some(String::new()),
            Tag::FootnoteDefinition(label) => {
                self.reading = Some(((label.to_string(), Vec::new()), 0));
            }
            Tag::Emphasis => self.style.italic = true,
            Tag::Strong => self.style.bold = true,
            Tag::Strikethrough => self.style.strike = true,
            Tag::Superscript => self.style.sup = true,
            Tag::Subscript => self.style.sub = true,
            Tag::Link { link_type, dest_url, .. } => {
                let dest = match link_type {
                    LinkType::WikiLink { .. } => wikilink(&dest_url),
                    _ => dest_url.to_string(),
                };
                self.link = Some(dest);
                self.style.link = true;
            }
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            Tag::Table(aligns) => {
                self.table = Some(TableState { aligns, rows: Vec::new() });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.rows.push((true, Vec::new()));
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push((false, Vec::new()));
                }
            }
            Tag::MetadataBlock(_) => self.metadata = Some(String::new()),
            Tag::DefinitionListTitle => self.style.bold = true,
            Tag::DefinitionListDefinition => {
                self.flush_paragraph(false);
                self.definitions_depth += 1;
            }
            Tag::Paragraph | Tag::TableCell | Tag::DefinitionList => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Heading(_) => self.heading_end(),
            TagEnd::Paragraph => self.paragraph_end(),
            TagEnd::List(_) => {
                self.flush_item();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.b.y += self.gap();
                }
            }
            TagEnd::Item => self.flush_item(),
            TagEnd::BlockQuote(_) => self.quote_end(),
            TagEnd::CodeBlock => {
                if let Some((lang, body, start)) = self.code.take() {
                    let x0 = self.indent();
                    if lang.eq_ignore_ascii_case("math") {
                        self.display_math(&body);
                        return;
                    }
                    if lang.eq_ignore_ascii_case("mermaid") {
                        let line = self.source[..start].matches('\n').count();
                        crate::diagrams::block(self, &body, line, x0);
                        self.b.y += self.gap();
                        return;
                    }
                    let highlight = self.highlight(&lang, &body);
                    blocks::code_block(
                        &mut self.b,
                        &lang,
                        &body,
                        highlight.as_deref().map(Vec::as_slice),
                        x0,
                    );
                    self.b.y += self.gap();
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html_block.take() {
                    self.html(&html);
                    self.flush_paragraph(true);
                }
            }
            TagEnd::Emphasis => self.style.italic = false,
            TagEnd::Strong => self.style.bold = false,
            TagEnd::Strikethrough => self.style.strike = false,
            TagEnd::Superscript => self.style.sup = false,
            TagEnd::Subscript => self.style.sub = false,
            TagEnd::Link => {
                self.link = None;
                self.style.link = false;
            }
            TagEnd::Image => self.image_end(),
            TagEnd::TableCell => {
                if let Some((_, row)) = self.table.as_mut().and_then(|t| t.rows.last_mut()) {
                    row.push(std::mem::take(&mut self.spans));
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    let x0 = self.indent();
                    blocks::table(&mut self.b, &t, x0);
                    self.b.y += self.gap();
                }
            }
            TagEnd::MetadataBlock(_) => {
                if let Some(text) = self.metadata.take() {
                    let x0 = self.indent();
                    if blocks::front_matter(&mut self.b, &text, x0) {
                        self.b.y += self.gap();
                    }
                }
            }
            TagEnd::DefinitionListTitle => {
                self.style.bold = false;
                self.flush_paragraph(false);
                self.b.y += self.b.px(2.0);
            }
            TagEnd::DefinitionListDefinition => {
                self.flush_paragraph(false);
                self.definitions_depth = self.definitions_depth.saturating_sub(1);
                self.b.y += self.b.px(4.0);
            }
            TagEnd::DefinitionList => self.b.y += self.gap(),
            TagEnd::TableHead | TagEnd::TableRow | TagEnd::FootnoteDefinition => {}
        }
    }

    /// A code block's highlighting by the editor's grammars, once worked out (`None`
    /// meanwhile, or without a grammar: colored by words).
    fn highlight(&mut self, lang: &str, body: &str) -> crate::layout::Highlight {
        let extension = blocks::extension(lang).to_owned();
        if extension.is_empty() || blocks::plain(lang) {
            return None;
        }
        let highlighter = ui_components::snippets::highlighter()?;
        let key = crate::jobs::key(&(&extension, body));
        let body = body.to_owned();
        self.caches
            .code
            .get(key, move || highlighter.highlight(&extension, &body).map(Arc::new))
            .flatten()
    }

    fn heading_start(&mut self, level: HeadingLevel) {
        self.flush_paragraph(false);
        let level = level_number(level);
        self.block = Block::Heading(level);
        if self.b.y > self.b.px(30.0) {
            self.b.y += self.b.px(if level <= 2 { 14.0 } else { 8.0 });
        }
        if let Some(h) = self.outline.headings.get(self.heading) {
            self.b.anchors.push((h.anchor.clone(), self.b.y));
        }
        self.heading += 1;
    }

    fn heading_end(&mut self) {
        let Block::Heading(level) = self.block else { return };
        let factor = [2.0, 1.55, 1.28, 1.1, 1.0, 0.92][usize::from(level - 1)];
        let size = (self.b.body * factor).round();
        let x0 = self.indent();
        let (right, text_color, align) = (self.b.right, self.b.palette.text, self.align());
        let flowed = std::mem::take(&mut self.spans);
        self.b.flow(
            &flowed,
            Run {
                x0,
                x1: right,
                size,
                weight: Font::SEMIBOLD,
                color: text_color,
                line: 1.35,
                align,
            },
        );
        if level <= 2 {
            self.b.y += self.b.px(6.0);
            self.b.rule(x0, right);
        }
        self.b.y += self.gap();
        self.block = Block::Paragraph;
    }

    fn paragraph_end(&mut self) {
        self.callout_line = false;
        if self.is_toc() {
            self.spans.clear();
            self.toc();
            return;
        }
        let x0 = self.indent();
        if let Some(marker) = self.item_marker.take() {
            self.marker(marker, x0);
        }
        let color = self.text_color();
        self.flow_body(x0, color);
        self.b.y += if self.lists.is_empty() && self.definitions_depth == 0 {
            self.gap()
        } else {
            self.b.px(4.0)
        };
    }

    /// The pending text as a paragraph of its own (before a block that starts without
    /// closing it: a heading or a table in HTML, a code block in a list item).
    fn flush_paragraph(&mut self, gap: bool) {
        if self.spans.iter().all(|s| s.image.is_none() && s.text.trim().is_empty()) {
            self.spans.clear();
            return;
        }
        let x0 = self.indent();
        if let Some(marker) = self.item_marker.take() {
            self.marker(marker, x0);
        }
        let color = self.text_color();
        self.flow_body(x0, color);
        if gap {
            self.b.y += self.gap();
        }
    }

    fn text(&mut self, t: &str) {
        if let Some((_, body, _)) = &mut self.code {
            body.push_str(t);
        } else if let Some((_, alt)) = &mut self.image {
            alt.push_str(t);
        } else if let Some(meta) = &mut self.metadata {
            meta.push_str(t);
        } else if self.summary.is_some() || self.style.code {
            self.push(t, self.style);
        } else {
            let text = inline::emojify(t);
            self.plain(&text);
        }
    }

    /// Plain text: bare URLs as links (outside links), `==marks==`.
    fn plain(&mut self, text: &str) {
        if self.link.is_some() {
            return self.marks(text);
        }
        for piece in inline::autolinks(text) {
            match piece {
                Piece::Text(t) => self.marks(t),
                Piece::Link(shown, dest) => {
                    let style = Style { link: true, ..self.style };
                    if !self.callout_line {
                        self.spans.push(Span::text(shown, style, Some(dest)));
                    }
                }
            }
        }
    }

    /// `==text==` highlighted (Obsidian): only within one run of text, the marks touching
    /// their text (`a == b` stays as written).
    fn marks(&mut self, text: &str) {
        let mut rest = text;
        while let Some(open) = rest.find("==") {
            let inner = &rest[open + 2..];
            let Some(close) = inner.find("==") else { break };
            let marked = &inner[..close];
            if marked.is_empty() || marked.starts_with(' ') || marked.ends_with(' ') {
                self.push(&rest[..open + 2], self.style);
                rest = inner;
                continue;
            }
            self.push(&rest[..open], self.style);
            self.push(marked, Style { mark: true, ..self.style });
            rest = &inner[close + 2..];
        }
        self.push(rest, self.style);
    }
}

/// Runs of whitespace (new lines too) as one space.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// A wikilink's page (`[[Some page]]`, `[[page#heading]]`) as a path: a Markdown file next
/// to this one when it names no extension.
fn wikilink(dest: &str) -> String {
    let (page, anchor) = match dest.split_once('#') {
        Some((page, anchor)) => (page, Some(anchor)),
        None => (dest, None),
    };
    if page.is_empty() {
        return format!("#{}", anchor.unwrap_or_default());
    }
    let mut path = page.to_owned();
    if Path::new(page).extension().is_none() {
        path.push_str(".md");
    }
    path
}

/// An Obsidian callout's first line (`> [!info]- Title`): its kind, its title (if it has
/// one) and whether it folds (`-`: closed, `+`: open).
fn callout(quote: &str) -> Option<(Alert, Option<String>, Option<bool>)> {
    let line = quote.lines().next()?;
    let line = line.trim_start().strip_prefix('>')?.trim_start();
    let inner = line.strip_prefix("[!")?;
    let close = inner.find(']')?;
    let name = &inner[..close];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let mut rest = &inner[close + 1..];
    let fold = match rest.chars().next() {
        Some('-') => Some(false),
        Some('+') => Some(true),
        _ => None,
    };
    if fold.is_some() {
        rest = &rest[1..];
    }
    let title = rest.trim();
    let title = (!title.is_empty()).then(|| title.to_owned()).or_else(|| {
        let mut chars = name.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect())
    });
    Some((Alert::from_callout(name), title, fold))
}

#[cfg(test)]
#[path = "../tests/unit/walk.rs"]
mod tests;
