//! Mermaid: the diagram's type (its first word), its front matter (`---\ntitle: …\n---`)
//! and accessibility lines, then each type's own parser. What a type says it doesn't know
//! is an error with its line; the types not parsed yet are named as such
//! ([`ErrorKind::Unsupported`]).

pub mod block;
pub mod boxes;
pub mod class;
pub mod er;
pub mod flowchart;
pub mod gantt;
pub mod git;
pub mod journey;
pub mod mindmap;
pub mod packet;
pub mod pie;
pub mod quadrant;
pub mod radar;
pub mod sankey;
pub mod sequence;
pub mod state;
pub mod timeline;
pub mod tree;
pub mod xychart;

use kernel::Rgba;

pub use class::ClassDiagram;
pub use er::ErDiagram;
pub use flowchart::Flowchart;
pub use gantt::Gantt;
pub use git::GitGraph;
pub use mindmap::Mindmap;
pub use pie::Pie;
pub use sequence::Sequence;
pub use state::StateDiagram;
pub use timeline::Timeline;

/// A diagram, by type.
#[derive(Debug, Clone, PartialEq)]
pub enum Diagram {
    Flowchart(Flowchart),
    Sequence(Sequence),
    Class(ClassDiagram),
    State(StateDiagram),
    Er(ErDiagram),
    Gantt(Gantt),
    Pie(Pie),
    Mindmap(Mindmap),
    Timeline(Timeline),
    Git(GitGraph),
    Journey(journey::Journey),
    Sankey(sankey::Sankey),
    XyChart(xychart::XyChart),
    Quadrant(quadrant::Quadrant),
    Requirement(boxes::BoxGraph),
    C4(boxes::BoxGraph),
    Architecture(boxes::BoxGraph),
    Block(block::BlockDiagram),
    Packet(packet::Packet),
    Kanban(tree::Kanban),
    Radar(radar::Radar),
    Treemap(tree::Treemap),
}

/// What a diagram says besides itself: its title and its accessible description.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub meta: Meta,
    pub diagram: Diagram,
}

/// Why a diagram couldn't be read, and where (`line`: 1-based in the block; 0: the whole
/// block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Nothing but comments.
    Empty,
    /// A Mermaid type this doesn't draw yet (`journey`, `sankey-beta`).
    Unsupported(String),
    /// The first word names no diagram type.
    UnknownType(String),
    /// A statement not understood (its text).
    Unexpected(String),
    /// A block (`subgraph`, `loop`, `{`) never closed; what closes it.
    Unclosed(&'static str),
}

impl Error {
    pub fn new(line: usize, kind: ErrorKind) -> Self {
        Self { line, kind }
    }

    pub(crate) fn unexpected(line: &Line<'_>) -> Self {
        Self::new(line.number, ErrorKind::Unexpected(line.text.trim().to_owned()))
    }
}

/// A line of the block, its comments left out: its number (1-based) and its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<'a> {
    pub number: usize,
    pub text: &'a str,
}

/// The types Mermaid has that aren't parsed here yet.
const UNSUPPORTED: &[&str] = &["zenuml"];

/// Reads a Mermaid block.
pub fn parse(source: &str) -> Result<Parsed, Error> {
    let mut meta = Meta::default();
    let lines = body_lines(source, &mut meta);
    let Some(first) = lines.iter().position(|l| !l.text.trim().is_empty()) else {
        return Err(Error::new(0, ErrorKind::Empty));
    };
    let head = lines[first];
    let keyword = head.text.split_whitespace().next().unwrap_or_default();
    let rest = &lines[first + 1..];
    // The accessibility lines may be anywhere: taken out first.
    let (rest, accessible) = accessibility(rest);
    if let Some(t) = accessible.title {
        meta.title.get_or_insert(t);
    }
    if accessible.description.is_some() {
        meta.description = accessible.description;
    }
    let diagram = match keyword {
        "flowchart" | "graph" | "flowchart-elk" => {
            Diagram::Flowchart(flowchart::parse(head, &rest)?)
        }
        "sequenceDiagram" => Diagram::Sequence(sequence::parse(&rest, &mut meta)?),
        "classDiagram" | "classDiagram-v2" => Diagram::Class(class::parse(&rest)?),
        "stateDiagram" | "stateDiagram-v2" => Diagram::State(state::parse(&rest)?),
        "erDiagram" => Diagram::Er(er::parse(&rest)?),
        "gantt" => Diagram::Gantt(gantt::parse(&rest, &mut meta)?),
        "pie" => Diagram::Pie(pie::parse(head, &rest, &mut meta)?),
        "mindmap" => Diagram::Mindmap(mindmap::parse(&rest)?),
        "timeline" => Diagram::Timeline(timeline::parse(&rest, &mut meta)?),
        "gitGraph" | "gitGraph:" => Diagram::Git(git::parse(head, &rest)?),
        "journey" => Diagram::Journey(journey::parse(&rest, &mut meta)?),
        "sankey-beta" | "sankey" => Diagram::Sankey(sankey::parse(&rest)?),
        "xychart-beta" | "xychart" => Diagram::XyChart(xychart::parse(head, &rest, &mut meta)?),
        "quadrantChart" => Diagram::Quadrant(quadrant::parse(&rest, &mut meta)?),
        "requirementDiagram" => Diagram::Requirement(boxes::requirements(&rest)?),
        "C4Context" | "C4Container" | "C4Component" | "C4Dynamic" | "C4Deployment" => {
            Diagram::C4(boxes::c4(&rest, &mut meta)?)
        }
        "architecture-beta" | "architecture" => {
            Diagram::Architecture(boxes::architecture(&rest, &mut meta)?)
        }
        "block-beta" | "block" => Diagram::Block(block::parse(&rest)?),
        "packet-beta" | "packet" => Diagram::Packet(packet::parse(&rest, &mut meta)?),
        "kanban" => Diagram::Kanban(tree::parse(&rest, false)?),
        "radar-beta" | "radar" => Diagram::Radar(radar::parse(&rest, &mut meta)?),
        "treemap-beta" | "treemap" => Diagram::Treemap(tree::parse(&rest, true)?),
        k if UNSUPPORTED.contains(&k) => {
            return Err(Error::new(head.number, ErrorKind::Unsupported(k.to_owned())));
        }
        k => return Err(Error::new(head.number, ErrorKind::UnknownType(k.to_owned()))),
    };
    Ok(Parsed { meta, diagram })
}

/// The diagram's type as its first word names it (`flowchart`, `pie`), whatever else
/// the block holds.
pub fn kind(source: &str) -> Option<String> {
    let mut meta = Meta::default();
    body_lines(source, &mut meta)
        .iter()
        .find_map(|l| l.text.split_whitespace().next().map(str::to_owned))
}

/// The block's lines after its front matter (its `title` kept), without comments
/// (`%% …`) and directives (`%%{init: …}%%`).
fn body_lines<'a>(source: &'a str, meta: &mut Meta) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = source
        .lines()
        .enumerate()
        .map(|(i, text)| Line { number: i + 1, text: text.trim_end_matches('\r') })
        .collect();
    // Front matter: `---` … `---` before the type.
    let first = lines.iter().position(|l| !l.text.trim().is_empty());
    if let Some(first) = first
        && lines[first].text.trim() == "---"
        && let Some(end) = lines[first + 1..].iter().position(|l| l.text.trim() == "---")
    {
        for line in &lines[first + 1..first + 1 + end] {
            if let Some(title) = line.text.trim().strip_prefix("title:") {
                meta.title = Some(unquote(title.trim()).to_owned());
            }
        }
        lines.drain(..first + 2 + end);
    }
    for line in &mut lines {
        let t = line.text.trim_start();
        if t.starts_with("%%") {
            line.text = "";
        }
    }
    lines
}

/// `accTitle: …`, `accDescr: …` and `accDescr { … }` taken out of `lines`.
fn accessibility<'a>(lines: &[Line<'a>]) -> (Vec<Line<'a>>, Meta) {
    let mut meta = Meta::default();
    let mut out = Vec::with_capacity(lines.len());
    let mut block: Option<String> = None;
    for line in lines {
        let t = line.text.trim();
        if let Some(text) = &mut block {
            if let Some(last) = t.strip_suffix('}') {
                text.push_str(last.trim());
                meta.description = Some(text.trim().to_owned());
                block = None;
            } else {
                text.push_str(t);
                text.push(' ');
            }
            continue;
        }
        if let Some(title) =
            t.strip_prefix("accTitle").and_then(|r| r.trim_start().strip_prefix(':'))
        {
            meta.title = Some(title.trim().to_owned());
        } else if let Some(d) =
            t.strip_prefix("accDescr").and_then(|r| r.trim_start().strip_prefix(':'))
        {
            meta.description = Some(d.trim().to_owned());
        } else if let Some(r) = t.strip_prefix("accDescr").map(str::trim_start)
            && let Some(r) = r.strip_prefix('{')
        {
            block = Some(format!("{} ", r.trim()));
        } else {
            out.push(*line);
        }
    }
    (out, meta)
}

/// `"text"` without its quotes (anything else as it is).
pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
}

/// A label as written (`"quoted"`, `<br>` for a new line, `#quot;` entities, a Markdown
/// string's backticks) as the text it shows.
pub fn label(s: &str) -> String {
    let s = unquote(s);
    let s = s.strip_prefix('`').and_then(|s| s.strip_suffix('`')).unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let lower = rest.get(..5).map(str::to_ascii_lowercase);
        if lower.as_deref().is_some_and(|l| l.starts_with("<br"))
            && let Some(end) = rest.find('>')
        {
            out.push('\n');
            rest = &rest[end + 1..];
            continue;
        }
        if let Some(after) = rest.strip_prefix('#')
            && let Some(end) = after.find(';').filter(|&e| e <= 8)
            && let Some(c) = entity(&after[..end])
        {
            out.push(c);
            rest = &after[end + 1..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("\\n") {
            out.push('\n');
            rest = after;
            continue;
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn entity(name: &str) -> Option<char> {
    if let Ok(code) = name.parse::<u32>() {
        return char::from_u32(code);
    }
    Some(match name {
        "quot" => '"',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "nbsp" => ' ',
        "apos" => '\'',
        _ => return None,
    })
}

/// The look a diagram's statements give a box or a line (`fill:#f9f,stroke:#333,…`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Style {
    pub fill: Option<Rgba>,
    pub stroke: Option<Rgba>,
    pub stroke_width: Option<f32>,
    pub color: Option<Rgba>,
    pub dashed: bool,
    pub bold: bool,
    pub italic: bool,
}

impl Style {
    /// `fill:#f9f,stroke:#333,stroke-width:4px` (unknown properties ignored).
    pub fn parse(s: &str) -> Self {
        let mut style = Self::default();
        style.apply(s);
        style
    }

    /// The properties of `s` over these.
    pub fn apply(&mut self, s: &str) {
        for part in split_top(s, ',') {
            let Some((key, value)) = part.split_once(':') else { continue };
            let value = value.trim().trim_end_matches(';').trim();
            match key.trim() {
                "fill" | "background" | "background-color" => self.fill = color(value),
                "stroke" | "border-color" => self.stroke = color(value),
                "stroke-width" => {
                    self.stroke_width = value.trim_end_matches("px").trim().parse().ok();
                }
                "color" => self.color = color(value),
                "stroke-dasharray" => self.dashed = !value.is_empty() && value != "0",
                "font-weight" => {
                    self.bold = matches!(value, "bold" | "bolder" | "600" | "700" | "800" | "900")
                }
                "font-style" => self.italic = value == "italic",
                _ => {}
            }
        }
    }

    /// These properties, then `over`'s where it has them.
    pub fn merged(self, over: &Style) -> Style {
        Style {
            fill: over.fill.or(self.fill),
            stroke: over.stroke.or(self.stroke),
            stroke_width: over.stroke_width.or(self.stroke_width),
            color: over.color.or(self.color),
            dashed: self.dashed || over.dashed,
            bold: self.bold || over.bold,
            italic: self.italic || over.italic,
        }
    }
}

/// `s` split at `sep` outside parentheses (`rgb(1,2,3)` stays whole).
pub(crate) fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// A CSS color: `#rgb`, `#rrggbb`, `rgb(…)`, `rgba(…)`, `hsl(…)` or a common name.
pub fn color(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_end_matches("!important").trim();
    if s.starts_with('#') {
        return Rgba::from_hex(s);
    }
    let lower = s.to_ascii_lowercase();
    if let Some(args) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")) {
        let n: Vec<f32> = args
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.trim().trim_end_matches('%').parse().ok())
            .collect();
        if n.len() >= 3 {
            let a = n.get(3).copied().unwrap_or(1.0);
            return Some(Rgba::new(n[0] / 255.0, n[1] / 255.0, n[2] / 255.0, a.min(1.0)));
        }
        return None;
    }
    if let Some(args) = lower.strip_prefix("hsla(").or_else(|| lower.strip_prefix("hsl(")) {
        let n: Vec<f32> = args
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.trim().trim_end_matches("deg").trim_end_matches('%').parse().ok())
            .collect();
        if n.len() >= 3 {
            let a = n.get(3).copied().unwrap_or(1.0).min(1.0);
            return Some(Rgba::from_hsl(n[0], n[1] / 100.0, n[2] / 100.0, a));
        }
        return None;
    }
    let hex = match lower.as_str() {
        "black" => 0x000000,
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x008000,
        "lime" => 0x00ff00,
        "blue" => 0x0000ff,
        "yellow" => 0xffff00,
        "orange" => 0xffa500,
        "purple" => 0x800080,
        "pink" => 0xffc0cb,
        "gray" | "grey" => 0x808080,
        "lightgray" | "lightgrey" => 0xd3d3d3,
        "darkgray" | "darkgrey" => 0xa9a9a9,
        "lightblue" => 0xadd8e6,
        "lightgreen" => 0x90ee90,
        "lightyellow" => 0xffffe0,
        "darkblue" => 0x00008b,
        "darkgreen" => 0x006400,
        "darkred" => 0x8b0000,
        "cyan" | "aqua" => 0x00ffff,
        "magenta" | "fuchsia" => 0xff00ff,
        "brown" => 0xa52a2a,
        "navy" => 0x000080,
        "teal" => 0x008080,
        "olive" => 0x808000,
        "maroon" => 0x800000,
        "silver" => 0xc0c0c0,
        "gold" => 0xffd700,
        "coral" => 0xff7f50,
        "salmon" => 0xfa8072,
        "violet" => 0xee82ee,
        "indigo" => 0x4b0082,
        "tomato" => 0xff6347,
        "khaki" => 0xf0e68c,
        "beige" => 0xf5f5dc,
        "lavender" => 0xe6e6fa,
        "skyblue" => 0x87ceeb,
        "steelblue" => 0x4682b4,
        "transparent" | "none" => return Some(Rgba::TRANSPARENT),
        _ => return None,
    };
    Some(Rgba::rgb(hex))
}

/// A diagram's direction (`TD`, `LR`…).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Down,
    Up,
    Right,
    Left,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_uppercase().as_str() {
            "TB" | "TD" | "V" => Self::Down,
            "BT" | "^" => Self::Up,
            "LR" | ">" => Self::Right,
            "RL" | "<" => Self::Left,
            _ => return None,
        })
    }

    /// Its levels go across (left / right).
    pub fn horizontal(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/mod.rs"]
mod tests;
