//! `flowchart` / `graph`: nodes (their shapes and labels), links (lines, heads, labels,
//! lengths), subgraphs (nested, with their own direction), styles (`classDef`, `class`,
//! `:::`, `style`, `linkStyle`) and `click` links.

use std::collections::HashMap;

use super::{Direction, Error, ErrorKind, Line, Style, label};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
}

/// A box's shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Shape {
    #[default]
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DoubleCircle,
    /// `>text]`: a flag.
    Asymmetric,
    Diamond,
    Hexagon,
    /// `[/text/]`
    Parallelogram,
    /// `[\text\]`
    ParallelogramAlt,
    /// `[/text\]`
    Trapezoid,
    /// `[\text/]`
    TrapezoidAlt,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    pub style: Style,
    /// `click id href "url"`.
    pub link: Option<String>,
    pub tooltip: Option<String>,
    /// The line it's first named on.
    pub line: usize,
}

/// What a link ends at: a node, or a whole subgraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum End {
    Node(usize),
    Group(usize),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Stroke {
    #[default]
    Solid,
    Dotted,
    Thick,
    Invisible,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Head {
    #[default]
    None,
    Arrow,
    Circle,
    Cross,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: End,
    pub to: End,
    pub label: Option<String>,
    pub stroke: Stroke,
    /// At its target, and at its source (`<-->`).
    pub head: Head,
    pub tail: Head,
    /// How many levels it spans at least (`--->`: 2).
    pub length: usize,
    pub style: Style,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subgraph {
    pub id: String,
    pub title: String,
    /// Its own nodes and subgraphs (not theirs).
    pub nodes: Vec<usize>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub direction: Option<Direction>,
    pub style: Style,
    pub line: usize,
}

/// What the parser holds while it reads.
#[derive(Default)]
struct State {
    chart: Flowchart,
    ids: HashMap<String, usize>,
    classes: HashMap<String, Style>,
    /// Classes given to nodes, applied at the end (a `classDef` may come after).
    node_classes: Vec<(usize, String)>,
    group_classes: Vec<(usize, String)>,
    /// The subgraphs open, innermost last.
    open: Vec<usize>,
    styles: Vec<(String, Style)>,
    link_styles: Vec<(String, Style)>,
}

pub(super) fn parse(head: Line<'_>, lines: &[Line<'_>]) -> Result<Flowchart, Error> {
    let mut state = State::default();
    let mut words = head.text.split_whitespace();
    words.next();
    if let Some(d) = words.next() {
        state.chart.direction = Direction::parse(d).ok_or_else(|| Error::unexpected(&head))?;
    }
    // A first line may hold statements after the direction (`graph LR; A-->B`).
    let after = head.text.find(';').map(|i| &head.text[i + 1..]).unwrap_or("");
    let mut all = vec![Line { number: head.number, text: after }];
    all.extend_from_slice(lines);
    for line in &all {
        for statement in statements(line.text) {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            state.statement(Line { number: line.number, text: statement })?;
        }
    }
    if let Some(&open) = state.open.last() {
        return Err(Error::new(state.chart.subgraphs[open].line, ErrorKind::Unclosed("end")));
    }
    state.finish();
    Ok(state.chart)
}

/// A line's statements (`;` between them, outside quotes and brackets).
fn statements(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

impl State {
    fn statement(&mut self, line: Line<'_>) -> Result<(), Error> {
        let t = line.text;
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "subgraph" => return self.subgraph(rest, line.number),
            "end" if rest.is_empty() => {
                return match self.open.pop() {
                    Some(_) => Ok(()),
                    None => Err(Error::unexpected(&line)),
                };
            }
            "direction" => {
                let d = Direction::parse(rest).ok_or_else(|| Error::unexpected(&line))?;
                match self.open.last() {
                    Some(&g) => self.chart.subgraphs[g].direction = Some(d),
                    None => self.chart.direction = d,
                }
                return Ok(());
            }
            "classDef" => {
                let (names, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                for name in names.split(',') {
                    let entry = self.classes.entry(name.trim().to_owned()).or_default();
                    entry.apply(style);
                }
                return Ok(());
            }
            "class" => {
                let (ids, class) = rest
                    .rsplit_once(char::is_whitespace)
                    .ok_or_else(|| Error::unexpected(&line))?;
                for id in ids.split(',') {
                    let id = id.trim();
                    match self.group(id) {
                        Some(g) => self.group_classes.push((g, class.trim().to_owned())),
                        None => {
                            let n = self.node(id, line.number);
                            self.node_classes.push((n, class.trim().to_owned()));
                        }
                    }
                }
                return Ok(());
            }
            "style" => {
                let (id, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                self.styles.push((id.to_owned(), Style::parse(style)));
                return Ok(());
            }
            "linkStyle" => {
                let (which, style) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                self.link_styles.push((which.to_owned(), Style::parse(style)));
                return Ok(());
            }
            "click" => return self.click(rest, &line),
            _ => {}
        }
        self.chain(&line)
    }

    fn subgraph(&mut self, rest: &str, number: usize) -> Result<(), Error> {
        // `subgraph id [title]`, `subgraph "title"`, `subgraph title words`.
        let (id, title) = match rest.find('[') {
            Some(i) if rest.ends_with(']') => {
                (rest[..i].trim().to_owned(), label(&rest[i + 1..rest.len() - 1]))
            }
            _ if rest.starts_with('"') => (label(rest), label(rest)),
            _ => {
                let id = rest.split_whitespace().next().unwrap_or_default();
                let id = if rest.contains(' ') { rest.to_owned() } else { id.to_owned() };
                (id.clone(), label(&id))
            }
        };
        let index = self.chart.subgraphs.len();
        let parent = self.open.last().copied();
        let id = if id.is_empty() { format!("subGraph{index}") } else { id };
        self.chart.subgraphs.push(Subgraph {
            id,
            title,
            parent,
            line: number,
            ..Subgraph::default()
        });
        if let Some(p) = parent {
            self.chart.subgraphs[p].children.push(index);
        }
        self.open.push(index);
        Ok(())
    }

    /// `click id "url" "tooltip"`, `click id href "url"`, `click id call fn()` (ignored).
    fn click(&mut self, rest: &str, line: &Line<'_>) -> Result<(), Error> {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let id = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();
        let rest = rest.strip_prefix("href").map(str::trim).unwrap_or(rest);
        if !rest.starts_with('"') {
            return Ok(());
        }
        let quoted: Vec<&str> = rest.split('"').skip(1).step_by(2).collect();
        let Some(&url) = quoted.first() else { return Err(Error::unexpected(line)) };
        let n = self.node(id, line.number);
        self.chart.nodes[n].link = Some(url.to_owned());
        self.chart.nodes[n].tooltip = quoted.get(1).map(|t| (*t).to_owned());
        Ok(())
    }

    fn group(&self, id: &str) -> Option<usize> {
        self.chart.subgraphs.iter().position(|g| g.id == id)
    }

    /// The node named `id`, made if it's new (in the subgraph open).
    fn node(&mut self, id: &str, number: usize) -> usize {
        if let Some(&n) = self.ids.get(id) {
            return n;
        }
        let n = self.chart.nodes.len();
        self.chart.nodes.push(Node {
            id: id.to_owned(),
            label: id.to_owned(),
            line: number,
            ..Node::default()
        });
        self.ids.insert(id.to_owned(), n);
        if let Some(&g) = self.open.last() {
            self.chart.subgraphs[g].nodes.push(n);
        }
        n
    }

    /// `A[x] --> B & C -.-> D`: nodes (or subgraphs) joined by links.
    fn chain(&mut self, line: &Line<'_>) -> Result<(), Error> {
        let mut s = line.text.trim();
        let mut previous: Vec<End> = Vec::new();
        let mut pending: Option<Link> = None;
        loop {
            let mut group = Vec::new();
            loop {
                let (end, rest) = self.reference(s, line)?;
                group.push(end);
                s = rest.trim_start();
                match s.strip_prefix('&') {
                    Some(r) => s = r.trim_start(),
                    None => break,
                }
            }
            if let Some(link) = pending.take() {
                for &from in &previous {
                    for &to in &group {
                        self.chart.edges.push(Edge {
                            from,
                            to,
                            label: link.label.clone(),
                            stroke: link.stroke,
                            head: link.head,
                            tail: link.tail,
                            length: link.length,
                            style: Style::default(),
                            line: line.number,
                        });
                    }
                }
            }
            previous = group;
            if s.is_empty() {
                return Ok(());
            }
            let (link, rest) = parse_link(s).ok_or_else(|| Error::unexpected(line))?;
            pending = Some(link);
            s = rest.trim_start();
            if s.is_empty() {
                return Err(Error::unexpected(line));
            }
        }
    }

    /// A node (with its shape and label, if given) or a subgraph's id at the start of
    /// `s`, and what follows.
    fn reference<'a>(&mut self, s: &'a str, line: &Line<'_>) -> Result<(End, &'a str), Error> {
        let len = id_len(s);
        if len == 0 {
            return Err(Error::unexpected(line));
        }
        let id = &s[..len];
        let mut rest = &s[len..];
        let shape = shape_at(rest);
        if shape.is_none()
            && let Some(g) = self.group(id)
        {
            return Ok((End::Group(g), rest));
        }
        let n = self.node(id, line.number);
        if let Some((shape, open, close)) = shape {
            let inner = &rest[open.len()..];
            let (text, after, closer) = match inner.strip_prefix('"') {
                Some(q) => {
                    let end = q.find('"').ok_or_else(|| Error::unexpected(line))?;
                    let after = q[end + 1..].trim_start();
                    let (ci, after) = close
                        .iter()
                        .enumerate()
                        .find_map(|(ci, c)| after.strip_prefix(c).map(|a| (ci, a)))
                        .ok_or_else(|| Error::unexpected(line))?;
                    (&inner[..end + 2], after, ci)
                }
                None => {
                    let (at, ci) = close
                        .iter()
                        .enumerate()
                        .filter_map(|(ci, c)| inner.find(c).map(|i| (i, ci)))
                        .min_by_key(|(i, _)| *i)
                        .ok_or_else(|| Error::new(line.number, ErrorKind::Unclosed(close[0])))?;
                    (&inner[..at], &inner[at + close[ci].len()..], ci)
                }
            };
            let shape = match (shape, closer) {
                (Shape::Parallelogram, 1) => Shape::Trapezoid,
                (Shape::ParallelogramAlt, 1) => Shape::TrapezoidAlt,
                (shape, _) => shape,
            };
            let node = &mut self.chart.nodes[n];
            node.label = label(text.trim());
            node.shape = shape;
            rest = after;
        }
        if let Some(r) = rest.strip_prefix(":::") {
            let len = id_len(r);
            self.node_classes.push((n, r[..len].to_owned()));
            rest = &r[len..];
        }
        Ok((End::Node(n), rest))
    }

    /// Classes and styles applied, now that every one is known.
    fn finish(&mut self) {
        let default = self.classes.get("default").copied();
        if let Some(d) = default {
            for node in &mut self.chart.nodes {
                node.style = d;
            }
        }
        for (n, class) in &self.node_classes {
            if let Some(style) = self.classes.get(class) {
                let node = &mut self.chart.nodes[*n];
                node.style = node.style.merged(style);
            }
        }
        for (g, class) in &self.group_classes {
            if let Some(style) = self.classes.get(class) {
                let group = &mut self.chart.subgraphs[*g];
                group.style = group.style.merged(style);
            }
        }
        for (id, style) in &self.styles {
            if let Some(&n) = self.ids.get(id) {
                let node = &mut self.chart.nodes[n];
                node.style = node.style.merged(style);
            } else if let Some(g) = self.chart.subgraphs.iter().position(|g| &g.id == id) {
                let group = &mut self.chart.subgraphs[g];
                group.style = group.style.merged(style);
            }
        }
        for (which, style) in &self.link_styles {
            let edges = &mut self.chart.edges;
            if which == "default" {
                for e in edges.iter_mut() {
                    e.style = e.style.merged(style);
                }
                continue;
            }
            for i in which.split(',').filter_map(|i| i.trim().parse::<usize>().ok()) {
                if let Some(e) = edges.get_mut(i) {
                    e.style = e.style.merged(style);
                }
            }
        }
    }
}

/// An id's length at the start of `s`: letters, digits, `_`, and `-` not starting a link.
fn id_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    for (at, c) in s.char_indices() {
        let ok = c.is_alphanumeric() || c == '_' || c == '$' || c == '\'' && at > 0;
        let dash = c == '-'
            && at > 0
            && !matches!(bytes.get(at + 1), Some(b'-' | b'.' | b'>' | b'=' | b'x' | b'o') | None)
            && bytes.get(at + 1).is_some_and(|b| b.is_ascii_alphanumeric());
        if ok || dash {
            i = at + c.len_utf8();
        } else {
            break;
        }
    }
    // A trailing `o` / `x` belongs to a link (`A--oB` is rare; `Ao--o` isn't an id).
    i
}

/// The shape opening at the start of `s`: the shape, its opener and its possible closers.
fn shape_at(s: &str) -> Option<(Shape, &'static str, &'static [&'static str])> {
    const SHAPES: &[(&str, Shape, &[&str])] = &[
        ("(((", Shape::DoubleCircle, &[")))"]),
        ("((", Shape::Circle, &["))"]),
        ("([", Shape::Stadium, &["])"]),
        ("[[", Shape::Subroutine, &["]]"]),
        ("[(", Shape::Cylinder, &[")]"]),
        ("[/", Shape::Parallelogram, &["/]", "\\]"]),
        ("[\\", Shape::ParallelogramAlt, &["\\]", "/]"]),
        ("{{", Shape::Hexagon, &["}}"]),
        ("(", Shape::Round, &[")"]),
        ("[", Shape::Rect, &["]"]),
        ("{", Shape::Diamond, &["}"]),
        (">", Shape::Asymmetric, &["]"]),
    ];
    SHAPES.iter().find(|(open, ..)| s.starts_with(open)).map(|(o, shape, c)| (*shape, *o, *c))
}

/// A link as read.
#[derive(Debug, Clone)]
struct Link {
    stroke: Stroke,
    head: Head,
    tail: Head,
    length: usize,
    label: Option<String>,
}

fn head_of(c: u8) -> Option<Head> {
    match c {
        b'>' | b'<' => Some(Head::Arrow),
        b'o' => Some(Head::Circle),
        b'x' => Some(Head::Cross),
        _ => None,
    }
}

/// A link at the start of `s` (`-->`, `-.->`, `==>`, `<-->`, `--o`, `---`, `~~~`, with
/// `|label|` or `-- label -->`), and what follows it.
fn parse_link(s: &str) -> Option<(Link, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut tail = Head::None;
    if let Some(h) = bytes.first().and_then(|&c| head_of(c))
        && bytes.get(1).is_some_and(|c| matches!(c, b'-' | b'=' | b'.'))
    {
        tail = h;
        i = 1;
    }
    let body_start = i;
    while i < bytes.len() && matches!(bytes[i], b'-' | b'=' | b'.' | b'~') {
        i += 1;
    }
    let body = &s[body_start..i];
    if body.is_empty() {
        return None;
    }
    let mut head = Head::None;
    if let Some(h) = bytes.get(i).and_then(|&c| head_of(c))
        && bytes[i] != b'<'
        && (bytes[i] == b'>' || body.len() >= 2)
    {
        head = h;
        i += 1;
    }
    let mut link =
        Link { stroke: stroke_of(body), head, tail, length: length_of(body), label: None };
    let mut rest = &s[i..];
    // `-- text -->`: the opening half, then the text up to the closing half.
    if head == Head::None
        && matches!(body, "--" | "==" | "-.")
        && rest.starts_with(char::is_whitespace)
    {
        let (text, after) = text_link(rest)?;
        let (closing, after) = parse_link(after)?;
        link.label = Some(label(text.trim()));
        link.head = closing.head;
        link.stroke = if body == "-." { Stroke::Dotted } else { closing.stroke };
        link.length = closing.length;
        return Some((link, after));
    }
    if body.len() < 2 && head == Head::None {
        return None;
    }
    let trimmed = rest.trim_start();
    if let Some(inner) = trimmed.strip_prefix('|') {
        let end = inner.find('|')?;
        link.label = Some(label(&inner[..end]));
        rest = &inner[end + 1..];
    }
    Some((link, rest))
}

/// The text of a `-- text -->` link: up to its closing half (a run of `-`, `=` or `.`
/// after a space), and what follows.
fn text_link(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i - 1] == b' '
            && matches!(bytes[i], b'-' | b'=' | b'.')
            && bytes.get(i + 1).is_some_and(|c| matches!(c, b'-' | b'=' | b'.' | b'>'))
        {
            return Some((&s[..i], &s[i..]));
        }
    }
    None
}

fn stroke_of(body: &str) -> Stroke {
    if body.contains('~') {
        Stroke::Invisible
    } else if body.contains('=') {
        Stroke::Thick
    } else if body.contains('.') {
        Stroke::Dotted
    } else {
        Stroke::Solid
    }
}

fn length_of(body: &str) -> usize {
    let n =
        if body.contains('.') { body.matches('.').count() } else { body.len().saturating_sub(1) };
    n.max(1)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/flowchart.rs"]
mod tests;
