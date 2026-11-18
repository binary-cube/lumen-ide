//! A cell's style as it's drawn: its shape with its proportions, its colors (draw.io's
//! `none` kept apart from "the default", which follows the theme), its line, its text's font
//! and alignment — with draw.io's defaults where the style says nothing.

use diagram_model::Style;
use kernel::Rgba;
use vector_tiles::ShapeKind;

/// A color a style gives: one, none at all, or the theme's default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    Default,
    None,
    Color(Rgba),
}

impl Paint {
    /// `key` of `style`.
    fn of(style: &Style, key: &str) -> Self {
        match style.get(key).map(str::trim) {
            None | Some("" | "default" | "inherit") => Self::Default,
            Some("none") => Self::None,
            Some(v) => color(v).map_or(Self::Default, Self::Color),
        }
    }

    /// The color drawn (`default`: the theme's).
    pub fn or(self, default: Rgba) -> Option<Rgba> {
        match self {
            Self::Default => Some(default),
            Self::None => None,
            Self::Color(c) => Some(c),
        }
    }
}

/// `#rrggbb` or `#rgb`.
pub fn color(text: &str) -> Option<Rgba> {
    let hex = text.strip_prefix('#')?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(Rgba::rgb(value)),
        3 => {
            let (r, g, b) = ((value >> 8) & 0xF, (value >> 4) & 0xF, value & 0xF);
            Some(Rgba::rgb(((r * 17) << 16) | ((g * 17) << 8) | (b * 17)))
        }
        _ => None,
    }
}

/// Where a text sits across its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Where a text sits down its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// How a cell is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// Its shape (`None`: none, only its text — a text, a group).
    pub shape: Option<ShapeKind>,
    /// A swimlane's header, this high.
    pub header: Option<f32>,
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f32,
    pub dashed: bool,
    pub font_size: f32,
    pub font_color: Paint,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    pub valign: VAlign,
    /// Its text wraps to its box.
    pub wrap: bool,
    /// Its text is HTML.
    pub html: bool,
}

/// A size given as a fraction of `length` (`fraction` when unset), or in pixels with
/// `fixedSize=1` (`pixels` when unset), as draw.io reads it.
fn size(style: &Style, key: &str, (fraction, pixels): (f64, f64), length: f32) -> f32 {
    let fixed = style.flag("fixedSize");
    let value = style.number(key).unwrap_or(if fixed { pixels } else { fraction }) as f32;
    if fixed || value > 1.0 { value } else { value * length }
}

/// `style`'s look for a box `w` × `h`.
pub fn look(style: &Style, w: f32, h: f32) -> Look {
    let name = style.shape();
    let swimlane = name == Some("swimlane") || style.has_name("swimlane");
    let shape = match name {
        _ if swimlane => Some(rect(style, w, h)),
        Some("text" | "edgeLabel" | "group") => None,
        Some("ellipse" | "doubleEllipse") => Some(ShapeKind::Ellipse),
        Some("rhombus") => Some(ShapeKind::Rhombus),
        Some("triangle") => Some(ShapeKind::Triangle),
        Some("parallelogram") => {
            Some(ShapeKind::Parallelogram { slant: size(style, "size", (0.2, 20.0), w) })
        }
        Some("hexagon") => Some(ShapeKind::Hexagon { inset: size(style, "size", (0.25, 20.0), w) }),
        Some("cylinder3" | "cylinder" | "datastore") => {
            Some(ShapeKind::Cylinder { depth: style.number("size").unwrap_or(15.0) as f32 })
        }
        Some("cloud") => Some(ShapeKind::Cloud),
        Some("document") => {
            Some(ShapeKind::Document { wave: style.number("size").unwrap_or(0.3) as f32 })
        }
        Some("process") => {
            Some(ShapeKind::Process { inset: style.number("size").unwrap_or(0.1) as f32 })
        }
        Some("actor" | "umlActor") => Some(ShapeKind::Actor),
        Some("note" | "note2") => {
            Some(ShapeKind::Note { fold: style.number("size").unwrap_or(30.0) as f32 })
        }
        Some("singleArrow") => Some(ShapeKind::Arrow {
            shaft: style.number("arrowWidth").unwrap_or(0.3) as f32,
            head: style.number("arrowSize").unwrap_or(0.2) as f32,
        }),
        Some("doubleArrow") => Some(ShapeKind::DoubleArrow {
            shaft: style.number("arrowWidth").unwrap_or(0.3) as f32,
            head: style.number("arrowSize").unwrap_or(0.2) as f32,
        }),
        // draw.io's own defaults for each (a fraction of a side, or pixels).
        Some("trapezoid") => {
            Some(ShapeKind::Trapezoid { inset: size(style, "size", (0.2, 20.0), w) })
        }
        Some("step") => Some(ShapeKind::Step { depth: size(style, "size", (0.2, 20.0), w) }),
        Some("card") => Some(ShapeKind::Card { cut: style.number("size").unwrap_or(30.0) as f32 }),
        Some("callout") => Some(ShapeKind::Callout {
            tail: style.number("size").unwrap_or(30.0) as f32,
            position: style.number("position").unwrap_or(0.5) as f32,
            tip: style.number("position2").unwrap_or(0.5) as f32,
            base: style.number("base").unwrap_or(20.0) as f32,
        }),
        Some("tape") => Some(ShapeKind::Tape { wave: style.number("size").unwrap_or(0.4) as f32 }),
        Some("delay") => Some(ShapeKind::Delay),
        Some("offPageConnector") => {
            Some(ShapeKind::OffPage { depth: size(style, "size", (0.375, 20.0), h) })
        }
        Some("manualInput") => {
            Some(ShapeKind::ManualInput { rise: style.number("size").unwrap_or(30.0) as f32 })
        }
        // A rectangle, or a shape not drawn here yet: its box (its text in it).
        _ => Some(rect(style, w, h)),
    };
    // A text draws no box unless its style gives it colors.
    let bare = shape.is_none();
    let paint = |key: &str| match Paint::of(style, key) {
        Paint::Default if bare => Paint::None,
        p => p,
    };
    let font_style = style.number("fontStyle").unwrap_or(0.0) as u32;
    Look {
        shape,
        header: swimlane.then(|| style.number("startSize").unwrap_or(23.0) as f32),
        fill: paint("fillColor"),
        stroke: paint("strokeColor"),
        stroke_width: style.number("strokeWidth").unwrap_or(1.0).clamp(0.0, 50.0) as f32,
        dashed: style.flag("dashed"),
        font_size: style.number("fontSize").unwrap_or(12.0).clamp(1.0, 400.0) as f32,
        font_color: Paint::of(style, "fontColor"),
        bold: font_style & 1 != 0,
        italic: font_style & 2 != 0,
        align: match style.get("align") {
            Some("left") => Align::Left,
            Some("right") => Align::Right,
            _ => Align::Center,
        },
        valign: match style.get("verticalAlign") {
            Some("top") => VAlign::Top,
            Some("bottom") => VAlign::Bottom,
            _ if swimlane => VAlign::Top,
            _ => VAlign::Middle,
        },
        wrap: style.get("whiteSpace") == Some("wrap"),
        html: style.flag("html"),
    }
}

/// A box, its corners rounded as draw.io rounds them (`arcSize`: a percentage of its
/// smaller side, 15 by default; half of it in pixels with `absoluteArcSize`).
fn rect(style: &Style, w: f32, h: f32) -> ShapeKind {
    if !style.flag("rounded") {
        return ShapeKind::Rect { radius: 0.0 };
    }
    let arc = style.number("arcSize").unwrap_or(15.0) as f32;
    let radius = if style.flag("absoluteArcSize") { arc / 2.0 } else { w.min(h) * arc / 100.0 };
    ShapeKind::Rect { radius }
}

/// An edge's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    None,
    /// draw.io's classic: a triangle notched at its back.
    Classic,
    /// A plain triangle.
    Block,
    /// Two lines.
    Open,
    Oval,
    Diamond,
}

/// An edge's look: its line, its ends.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeLook {
    pub stroke: Paint,
    pub width: f32,
    pub dashed: bool,
    pub start: (Marker, bool),
    pub end: (Marker, bool),
    /// Its ends' size.
    pub marker_size: f32,
    /// Routed at right angles.
    pub orthogonal: bool,
    /// Drawn as a smooth curve through its points.
    pub curved: bool,
}

fn marker(name: Option<&str>, default: Marker) -> Marker {
    match name {
        None => default,
        Some("none" | "") => Marker::None,
        Some("classic" | "classicThin") => Marker::Classic,
        Some("block" | "blockThin") => Marker::Block,
        Some("open" | "openThin" | "openAsync") => Marker::Open,
        Some("oval" | "circle" | "dash") => Marker::Oval,
        Some("diamond" | "diamondThin") => Marker::Diamond,
        // Another end draw.io has: an arrow, as its default.
        Some(_) => Marker::Classic,
    }
}

/// `style`'s look for an edge (draw.io's defaults: a classic arrow at its end).
pub fn edge_look(style: &Style) -> EdgeLook {
    let filled = |key| style.get(key) != Some("0");
    EdgeLook {
        stroke: Paint::of(style, "strokeColor"),
        width: style.number("strokeWidth").unwrap_or(1.0).clamp(0.0, 50.0) as f32,
        dashed: style.flag("dashed"),
        start: (marker(style.get("startArrow"), Marker::None), filled("startFill")),
        end: (marker(style.get("endArrow"), Marker::Classic), filled("endFill")),
        marker_size: style.number("endSize").unwrap_or(6.0).clamp(1.0, 60.0) as f32,
        orthogonal: matches!(
            style.get("edgeStyle"),
            Some("orthogonalEdgeStyle" | "elbowEdgeStyle" | "entityRelationEdgeStyle")
        ),
        curved: style.flag("curved"),
    }
}

#[cfg(test)]
#[path = "../tests/unit/look.rs"]
mod tests;
