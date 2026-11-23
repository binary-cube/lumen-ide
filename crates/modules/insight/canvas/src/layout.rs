//! Canvas layout (in its own coordinates, logical pixels at zoom 1): card columns and
//! edge endpoints. Computed once per graph / mode / selection.

use code_intelligence::{DepGraph, EdgeKind, SymbolKind};
use kernel::{Point, Rect};

pub const HEADER: f32 = 46.0;
pub const ROW: f32 = 26.0;
pub const FOOTER: f32 = 48.0;
/// How many code lines the selected declaration's card has.
pub const MAX_CODE: usize = 16;

const FILE_W: f32 = 300.0;
const FOCUS_W: f32 = 340.0;
const CODE_W: f32 = 420.0;
const SELECTED_W: f32 = 520.0;
const COLUMN_GAP: f32 = 130.0;
const ROW_GAP: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Declarations of the center file and their code.
    Functions,
    /// Files only.
    Files,
    /// Files with their first lines of code.
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CardKind {
    File {
        file: usize,
        focus: bool,
    },
    /// Code of a declaration in the center file.
    Code {
        symbol: usize,
    },
}

/// A row with a declaration in the center file's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymRow {
    pub symbol: usize,
    /// A class title (`class Car`), not a clickable row.
    pub title: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rows {
    /// The first `n` lines of code of the file.
    Preview(usize),
    /// `142 lines · unconnected`.
    Summary,
    Symbols(Vec<SymRow>),
}

#[derive(Debug, Clone)]
pub struct Card {
    pub kind: CardKind,
    pub rect: Rect,
    pub rows: Rows,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct EdgeLine {
    pub from: Point,
    pub to: Point,
    pub strong: bool,
    pub dashed: bool,
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub cards: Vec<Card>,
    pub edges: Vec<EdgeLine>,
    pub bounds: Rect,
}

fn card_height(rows: &Rows) -> f32 {
    let n = match rows {
        Rows::Preview(n) => *n,
        Rows::Summary => 1,
        Rows::Symbols(rows) => rows.len(),
    };
    HEADER + 10.0 + n as f32 * ROW + 14.0
}

/// Stacks a column's cards one below another, vertically centered on 0.
fn stack(cards: &mut [Card], x: f32) {
    let total: f32 = cards.iter().map(|c| c.rect.size.height).sum::<f32>()
        + ROW_GAP * cards.len().saturating_sub(1) as f32;
    let mut y = -total / 2.0;
    for card in cards {
        card.rect = Rect::new(x, y, card.rect.size.width, card.rect.size.height);
        y += card.rect.size.height + ROW_GAP;
    }
}

/// The cards of the canvas's columns, left to right.
struct Columns {
    /// Files that use the center file, then the unconnected ones.
    left: Vec<Card>,
    /// The center file (a single card).
    focus: Vec<Card>,
    /// The center file's declarations with their code ("Functions" mode).
    code: Vec<Card>,
    /// Files the center file uses.
    right: Vec<Card>,
}

impl Layout {
    pub fn new(
        graph: &DepGraph,
        mode: Mode,
        selected: Option<usize>,
        edit_lines: Option<usize>,
    ) -> Self {
        let (incoming, outgoing) = neighbors(graph);
        let (sym_rows, code_symbols) = focus_symbols(graph);
        let focus = focus_card(graph, mode, sym_rows);
        let code = if mode == Mode::Functions {
            code_cards(graph, &code_symbols, selected, edit_lines)
        } else {
            Vec::new()
        };
        let mut columns = Columns {
            left: left_column(graph, mode, &incoming),
            focus: vec![focus],
            code,
            right: outgoing.iter().map(|&i| file_card(graph, mode, i)).collect(),
        };
        columns.place();
        let edges = columns.edges(graph, selected);

        let Columns { left, focus, code, right } = columns;
        let mut cards = left;
        cards.extend(focus);
        cards.extend(code);
        cards.extend(right);
        let bounds = cards.iter().map(|c| c.rect).reduce(|a, b| a.union(&b)).unwrap_or_default();
        Self { cards, edges, bounds }
    }
}

/// The files that use the center file (incoming) and those it uses (outgoing, unless they
/// also use it).
fn neighbors(graph: &DepGraph) -> (Vec<usize>, Vec<usize>) {
    let focus = graph.focus;
    // One pass over the edges (not one per file), then the files in their order.
    let (mut uses, mut used) = (vec![false; graph.files.len()], vec![false; graph.files.len()]);
    for e in &graph.edges {
        if e.to == focus && e.from < uses.len() {
            uses[e.from] = true;
        }
        if e.from == focus && e.to < used.len() {
            used[e.to] = true;
        }
    }
    let incoming: Vec<usize> = (0..graph.files.len()).filter(|&i| i != focus && uses[i]).collect();
    let outgoing: Vec<usize> =
        (0..graph.files.len()).filter(|&i| i != focus && !uses[i] && used[i]).collect();
    (incoming, outgoing)
}

/// A neighbor file's card (not placed yet).
fn file_card(graph: &DepGraph, mode: Mode, i: usize) -> Card {
    let rows = match mode {
        Mode::Functions => Rows::Preview(graph.files[i].preview.len().min(6)),
        Mode::Files => Rows::Summary,
        Mode::Code => Rows::Preview(graph.files[i].preview.len().min(14)),
    };
    Card {
        kind: CardKind::File { file: i, focus: false },
        rect: Rect::new(0.0, 0.0, FILE_W, card_height(&rows)),
        rows,
        selected: false,
    }
}

/// The center file's rows (classes with their methods, and the functions) and the
/// declarations that get a code card.
fn focus_symbols(graph: &DepGraph) -> (Vec<SymRow>, Vec<usize>) {
    let symbols = &graph.files[graph.focus].symbols;
    let mut sym_rows = Vec::new();
    let mut code_symbols = Vec::new();
    // Each type's methods, in their order, found in one pass (not all symbols per type).
    let mut methods: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (j, m) in symbols.iter().enumerate() {
        if m.kind == SymbolKind::Method
            && let Some(container) = m.container.as_deref()
        {
            methods.entry(container).or_default().push(j);
        }
    }
    for (i, s) in symbols.iter().enumerate() {
        if s.kind.is_type() {
            sym_rows.push(SymRow { symbol: i, title: true });
            for &j in methods.get(s.name.as_str()).into_iter().flatten() {
                sym_rows.push(SymRow { symbol: j, title: false });
                code_symbols.push(j);
            }
        } else if s.kind == SymbolKind::Function {
            sym_rows.push(SymRow { symbol: i, title: false });
            code_symbols.push(i);
        }
    }
    (sym_rows, code_symbols)
}

/// Center file: its declarations (in "Functions" mode), otherwise its code or a summary.
fn focus_card(graph: &DepGraph, mode: Mode, sym_rows: Vec<SymRow>) -> Card {
    let focus = graph.focus;
    let rows = match mode {
        Mode::Functions if !sym_rows.is_empty() => Rows::Symbols(sym_rows),
        Mode::Code | Mode::Functions => Rows::Preview(graph.files[focus].preview.len().min(14)),
        Mode::Files => Rows::Summary,
    };
    Card {
        kind: CardKind::File { file: focus, focus: true },
        rect: Rect::new(0.0, 0.0, FOCUS_W, card_height(&rows)),
        rows,
        selected: false,
    }
}

/// The files that use the center file, then the unconnected ones.
fn left_column(graph: &DepGraph, mode: Mode, incoming: &[usize]) -> Vec<Card> {
    let mut left: Vec<Card> = incoming.iter().map(|&i| file_card(graph, mode, i)).collect();
    // Unconnected files from the same directory, compact, below those that use it.
    for (i, f) in graph.files.iter().enumerate() {
        if f.unconnected {
            left.push(Card {
                kind: CardKind::File { file: i, focus: false },
                rect: Rect::new(0.0, 0.0, FILE_W, card_height(&Rows::Summary)),
                rows: Rows::Summary,
                selected: false,
            });
        }
    }
    left
}

/// The code cards of the center file's declarations: the selected one with its code (or
/// the lines being edited) and its callers' footer, the others with a few lines.
fn code_cards(
    graph: &DepGraph,
    code_symbols: &[usize],
    selected: Option<usize>,
    edit_lines: Option<usize>,
) -> Vec<Card> {
    let focus = graph.focus;
    let symbols = &graph.files[focus].symbols;
    let mut code = Vec::new();
    for &i in code_symbols {
        let s = &symbols[i];
        let is_selected = selected == Some(i);
        let height = if is_selected {
            let callers =
                graph.edges.iter().any(|e| e.to == focus && e.symbol == Some(i) && e.from != focus);
            HEADER
                + 10.0
                + edit_lines.unwrap_or_else(|| s.code.len().min(MAX_CODE)) as f32 * ROW
                + 14.0
                + if callers { FOOTER } else { 0.0 }
        } else {
            let lines =
                s.code.iter().filter(|l| !l.trim_start().starts_with(['*', '/'])).take(3).count();
            10.0 + lines.max(1) as f32 * ROW + 14.0
        };
        code.push(Card {
            kind: CardKind::Code { symbol: i },
            rect: Rect::new(0.0, 0.0, if is_selected { SELECTED_W } else { CODE_W }, height),
            rows: Rows::Preview(0),
            selected: is_selected,
        });
    }
    code
}

fn header_mid(r: Rect) -> f32 {
    r.min_y() + HEADER / 2.0
}

impl Columns {
    /// Places the columns side by side, each stacked around 0.
    fn place(&mut self) {
        let focus_x = if self.left.is_empty() { 0.0 } else { FILE_W + COLUMN_GAP };
        stack(&mut self.left, 0.0);
        stack(&mut self.focus, focus_x);
        let code_x = focus_x + FOCUS_W + COLUMN_GAP;
        stack(&mut self.code, code_x);
        let right_x = if self.code.is_empty() { code_x } else { code_x + SELECTED_W + COLUMN_GAP };
        stack(&mut self.right, right_x);
    }

    /// The middle of the center card's row for `symbol` (when it lists declarations).
    fn row_y(&self, symbol: usize) -> Option<f32> {
        let card = &self.focus[0];
        let Rows::Symbols(rows) = &card.rows else { return None };
        let i = rows.iter().position(|r| r.symbol == symbol && !r.title)?;
        Some(card.rect.min_y() + HEADER + 10.0 + i as f32 * ROW + ROW / 2.0)
    }

    fn edges(&self, graph: &DepGraph, selected: Option<usize>) -> Vec<EdgeLine> {
        let mut edges = Vec::new();
        self.left_edges(graph, selected, &mut edges);
        self.code_edges(&mut edges);
        self.right_edges(graph, &mut edges);
        edges
    }

    /// From the files that use the center file to the rows they use (or its header).
    fn left_edges(&self, graph: &DepGraph, selected: Option<usize>, edges: &mut Vec<EdgeLine>) {
        let focus = graph.focus;
        let focus_rect = self.focus[0].rect;
        for card in &self.left {
            let CardKind::File { file, .. } = card.kind else { continue };
            let mut seen = Vec::new();
            for e in graph.edges.iter().filter(|e| e.from == file && e.to == focus) {
                let target_y =
                    e.symbol.and_then(|s| self.row_y(s)).unwrap_or_else(|| header_mid(focus_rect));
                if seen.contains(&target_y) {
                    continue;
                }
                seen.push(target_y);
                // The preview row where the reference is (if visible).
                let shown = match card.rows {
                    Rows::Preview(n) => n,
                    _ => 0,
                };
                let start = graph.files[file].preview_start;
                let line = e.line.checked_sub(start).map_or(usize::MAX, |l| l as usize);
                let from_y = if line < shown {
                    card.rect.min_y() + HEADER + 10.0 + line as f32 * ROW + ROW / 2.0
                } else {
                    header_mid(card.rect)
                };
                edges.push(EdgeLine {
                    from: Point::new(card.rect.max_x(), from_y),
                    to: Point::new(focus_rect.min_x(), target_y),
                    strong: e.symbol.is_some() && e.symbol == selected,
                    dashed: e.kind == EdgeKind::Include,
                });
            }
        }
    }

    /// From the center card's declaration rows to their code cards.
    fn code_edges(&self, edges: &mut Vec<EdgeLine>) {
        let focus_rect = self.focus[0].rect;
        for card in &self.code {
            let CardKind::Code { symbol } = card.kind else { continue };
            let Some(y) = self.row_y(symbol) else { continue };
            let to_y = if card.selected {
                header_mid(card.rect)
            } else {
                card.rect.min_y() + card.rect.size.height / 2.0
            };
            edges.push(EdgeLine {
                from: Point::new(focus_rect.max_x(), y),
                to: Point::new(card.rect.min_x(), to_y),
                strong: card.selected,
                dashed: false,
            });
        }
    }

    /// From the center card's header to the files it uses.
    fn right_edges(&self, graph: &DepGraph, edges: &mut Vec<EdgeLine>) {
        let focus = graph.focus;
        let focus_rect = self.focus[0].rect;
        for card in &self.right {
            let CardKind::File { file, .. } = card.kind else { continue };
            let kind = graph.edges.iter().find(|e| e.from == focus && e.to == file).map(|e| e.kind);
            edges.push(EdgeLine {
                from: Point::new(focus_rect.max_x(), header_mid(focus_rect)),
                to: Point::new(card.rect.min_x(), header_mid(card.rect)),
                strong: false,
                dashed: kind == Some(EdgeKind::Include),
            });
        }
    }
}

impl Layout {
    /// Moves the cards the user dragged (`offsets`, in canvas coordinates),
    /// with the edge endpoints attached to them.
    pub fn shift(&mut self, offsets: &std::collections::HashMap<CardKind, Point>) {
        if offsets.is_empty() {
            return;
        }
        let moved: Vec<(Rect, Point)> =
            self.cards.iter().filter_map(|c| offsets.get(&c.kind).map(|d| (c.rect, *d))).collect();
        // An edge endpoint is on the edge of its card (left or right).
        let delta = |p: Point| {
            moved
                .iter()
                .find(|(r, _)| {
                    p.x >= r.min_x() - 1.0
                        && p.x <= r.max_x() + 1.0
                        && p.y >= r.min_y() - 1.0
                        && p.y <= r.max_y() + 1.0
                })
                .map(|(_, d)| *d)
        };
        for edge in &mut self.edges {
            if let Some(d) = delta(edge.from) {
                edge.from = Point::new(edge.from.x + d.x, edge.from.y + d.y);
            }
            if let Some(d) = delta(edge.to) {
                edge.to = Point::new(edge.to.x + d.x, edge.to.y + d.y);
            }
        }
        for card in &mut self.cards {
            if let Some(d) = offsets.get(&card.kind) {
                card.rect = Rect::new(
                    card.rect.min_x() + d.x,
                    card.rect.min_y() + d.y,
                    card.rect.size.width,
                    card.rect.size.height,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
