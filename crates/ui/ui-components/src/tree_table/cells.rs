//! What a table is made of: its columns, its rows (a parent, a sign, a tint, a mark) and
//! their cells (text in pieces of their own weight, a number, a bar, a badge, an icon), the
//! inks they're drawn in.

use super::*;

/// Where a column's cells sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// A column: its title (and what it is, in its tooltip), where its cells sit, its width.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    pub title: String,
    pub tip: Option<String>,
    pub align: Align,
    /// The narrowest it's drawn (logical pixels).
    pub min_width: f32,
    /// Its width (logical pixels); the first column (the tree) takes what the others leave.
    pub width: f32,
}

impl TableColumn {
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self { title: title.into(), tip: None, align: Align::Left, min_width: 48.0, width }
    }

    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }

    pub fn with_tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub fn with_min(mut self, min: f32) -> Self {
        self.min_width = min;
        self
    }
}

/// A color as a row or a cell asks for it, worked out with the theme when drawn: a tone of
/// the design system, a severity's, or the heat of a share (per mille, 0–1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Tone(Tone),
    Severity(Level),
    Heat(u16),
}

impl Ink {
    /// The heat of share `t` (0..1).
    pub fn heat(t: f64) -> Self {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        Self::Heat((t * 1000.0).round() as u16)
    }

    pub fn color(self, p: &Palette) -> Rgba {
        match self {
            Ink::Tone(t) => t.color(p),
            Ink::Severity(l) => l.color(p),
            Ink::Heat(h) => p.heat(f32::from(h.min(1000)) / 1000.0),
        }
    }
}

impl From<Tone> for Ink {
    fn from(tone: Tone) -> Self {
        Ink::Tone(tone)
    }
}

impl From<Level> for Ink {
    fn from(level: Level) -> Self {
        Ink::Severity(level)
    }
}

/// How a piece of a cell's text is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStyle {
    Normal,
    Strong,
    Muted,
    /// In the code font (a condition, a name as written).
    Code,
    /// In a color of its own (a factor that's far off).
    Ink(Ink),
    /// Small, in a rounded pill of its color (`×120 ↑`, `+12 nodes`).
    Pill(Ink),
}

/// A piece of a cell's text.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

impl Span {
    pub fn new(text: impl Into<String>, style: SpanStyle) -> Self {
        Self { text: text.into(), style }
    }
}

/// What a cell shows.
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Empty,
    /// Text in pieces (`Seq Scan` strong, `on orders` normal, `(id = 1)` muted).
    Text(Vec<Span>),
    /// A number, written with its digits grouped and at most `decimals` after the point,
    /// and its unit after it, dimmed (`1 204 rows`, `0.42 ms`).
    Number {
        value: f64,
        decimals: u8,
        unit: Option<String>,
    },
    /// A share (`fraction` 0..1) as a bar in `ink`, its label beside it.
    Bar {
        fraction: f32,
        ink: Ink,
        label: String,
    },
    /// Two parts of one whole (`a` and `b`, 0..1 together), its label beside it.
    Split {
        a: f32,
        b: f32,
        label: String,
    },
    Badge {
        label: String,
        tone: Tone,
    },
    Icon {
        name: &'static str,
        tone: Tone,
    },
}

/// A cell: what it shows, and what its tooltip says (the exact value, what it means); an
/// icon before it, an outline around it (the figure a problem is about).
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub value: CellValue,
    pub tip: Option<String>,
    /// An icon before what it shows, in its color (a spill's disk).
    pub lead: Option<(&'static str, Ink)>,
    /// Outlined in this color (the figure a finding is about).
    pub outline: Option<Ink>,
}

impl TableCell {
    pub fn empty() -> Self {
        Self::of(CellValue::Empty)
    }

    /// A cell showing `value`.
    pub fn of(value: CellValue) -> Self {
        Self { value, tip: None, lead: None, outline: None }
    }

    /// A bar of `fraction` in `ink`, `label` beside it.
    pub fn bar(fraction: f32, ink: impl Into<Ink>, label: impl Into<String>) -> Self {
        Self::of(CellValue::Bar { fraction, ink: ink.into(), label: label.into() })
    }

    pub fn with_lead(mut self, icon: &'static str, ink: impl Into<Ink>) -> Self {
        self.lead = Some((icon, ink.into()));
        self
    }

    pub fn outlined(mut self, ink: Option<Ink>) -> Self {
        self.outline = ink;
        self
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::spans(vec![Span::new(text, SpanStyle::Normal)])
    }

    pub fn spans(spans: Vec<Span>) -> Self {
        Self::of(CellValue::Text(spans))
    }

    pub fn number(value: f64, decimals: u8, unit: Option<String>) -> Self {
        Self::of(CellValue::Number { value, decimals, unit })
    }

    pub fn with_tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    /// Its text, as copied (a bar's label, a number as written).
    pub fn plain(&self) -> String {
        match &self.value {
            CellValue::Empty => String::new(),
            CellValue::Text(spans) => spans.iter().map(|s| s.text.as_str()).collect(),
            CellValue::Number { value, decimals, unit } => {
                let n = number_text(*value, *decimals);
                match unit {
                    Some(u) => format!("{n} {u}"),
                    None => n,
                }
            }
            CellValue::Bar { label, .. } | CellValue::Split { label, .. } => label.clone(),
            CellValue::Badge { label, .. } => label.clone(),
            CellValue::Icon { name, .. } => (*name).to_owned(),
        }
    }
}

/// A row: its parent (an earlier row: rows come parents first), its icon, its cells (the
/// first one the tree's), how it stands out.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub parent: Option<usize>,
    pub icon: Option<&'static str>,
    pub cells: Vec<TableCell>,
    /// On the path marked (an accent stripe at its left, its first piece of text stronger).
    pub marked: bool,
    /// A sign in the gutter (its icon, its color) and what its tooltip says.
    pub sign: Option<(&'static str, Ink)>,
    pub sign_tip: Option<String>,
    /// A light background in this color.
    pub tint: Option<Ink>,
    /// Said after its text while it's folded (`+1 240 nodes · 3 %`).
    pub folded_note: Option<String>,
}

impl TableRow {
    pub fn new(parent: Option<usize>, cells: Vec<TableCell>) -> Self {
        Self {
            parent,
            icon: None,
            cells,
            marked: false,
            sign: None,
            sign_tip: None,
            tint: None,
            folded_note: None,
        }
    }

    /// A sign in the gutter, its tooltip.
    pub fn with_sign(mut self, icon: &'static str, ink: impl Into<Ink>, tip: String) -> Self {
        self.sign = Some((icon, ink.into()));
        self.sign_tip = Some(tip);
        self
    }

    pub fn tinted(mut self, ink: Option<Ink>) -> Self {
        self.tint = ink;
        self
    }

    pub fn with_folded_note(mut self, note: Option<String>) -> Self {
        self.folded_note = note;
        self
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn marked(mut self, marked: bool) -> Self {
        self.marked = marked;
        self
    }
}
