//! A cell's geometry (`<mxGeometry>`): a shape's box, or an edge's way — its points, its
//! ends when not on a shape, a label's offset. What it read is kept: written back unchanged
//! when nothing moved, else only its numbers (an attribute left out stays out while it's
//! draw.io's default).

use crate::xml::{Element, number};

/// A point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Relative to its parent (an edge's label: `x` along the edge, -1…1).
    pub relative: bool,
    /// An edge's points between its ends.
    pub points: Vec<Point>,
    /// An edge's ends when they're on no shape.
    pub source_point: Option<Point>,
    pub target_point: Option<Point>,
    /// A label's offset from where it would be.
    pub offset: Option<Point>,
}

/// A number attribute of `el` (draw.io's default when left out: 0).
fn num(el: &Element, name: &str) -> f64 {
    el.attr(name).and_then(|v| v.trim().parse().ok()).filter(|v: &f64| v.is_finite()).unwrap_or(0.0)
}

fn point(el: &Element) -> Point {
    Point::new(num(el, "x"), num(el, "y"))
}

impl Geometry {
    /// `el` (an `mxGeometry`) read.
    pub(crate) fn read(el: &Element) -> Self {
        let points = el
            .child("Array", Some("points"))
            .map(|a| a.children.iter().filter(|p| p.name == "mxPoint").map(point).collect())
            .unwrap_or_default();
        let at = |as_: &str| el.child("mxPoint", Some(as_)).map(point);
        Self {
            x: num(el, "x"),
            y: num(el, "y"),
            width: num(el, "width"),
            height: num(el, "height"),
            relative: el.attr("relative") == Some("1"),
            points,
            source_point: at("sourcePoint"),
            target_point: at("targetPoint"),
            offset: at("offset"),
        }
    }

    /// Written into `el` (the `mxGeometry` it was read from, or a new one): its numbers set,
    /// those left out kept out while they're 0; its points made again only if they changed.
    pub(crate) fn write(&self, el: &mut Element, read: Option<&Geometry>) {
        if read == Some(self) {
            return;
        }
        for (name, value) in
            [("x", self.x), ("y", self.y), ("width", self.width), ("height", self.height)]
        {
            if value != 0.0 || el.attr(name).is_some() {
                el.set_attr(name, number(value));
            }
        }
        if self.relative {
            el.set_attr("relative", "1");
        } else {
            el.remove_attr("relative");
        }
        if read.is_none_or(|r| r.points != self.points) {
            el.children.retain(|c| !(c.name == "Array" && c.attr("as") == Some("points")));
            if !self.points.is_empty() {
                let mut array = Element::new("Array");
                array.set_attr("as", "points");
                array.children = self.points.iter().map(|p| point_el(*p, None)).collect();
                el.children.push(array);
            }
        }
        for (as_, value, before) in [
            ("sourcePoint", self.source_point, read.and_then(|r| r.source_point)),
            ("targetPoint", self.target_point, read.and_then(|r| r.target_point)),
            ("offset", self.offset, read.and_then(|r| r.offset)),
        ] {
            if read.is_some() && value == before {
                continue;
            }
            match (value, el.child_mut("mxPoint", Some(as_))) {
                (Some(p), Some(existing)) => set_point(existing, p),
                (Some(p), None) => el.children.push(point_el(p, Some(as_))),
                (None, _) => {
                    el.children.retain(|c| !(c.name == "mxPoint" && c.attr("as") == Some(as_)))
                }
            }
        }
    }

    /// Its box.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.x, self.y, self.width, self.height)
    }
}

fn set_point(el: &mut Element, p: Point) {
    for (name, value) in [("x", p.x), ("y", p.y)] {
        if value != 0.0 || el.attr(name).is_some() {
            el.set_attr(name, number(value));
        }
    }
}

/// An `mxPoint` (with its `as`, if it's one of a geometry's named points).
fn point_el(p: Point, as_: Option<&str>) -> Element {
    let mut el = Element::new("mxPoint");
    set_point(&mut el, p);
    if let Some(as_) = as_ {
        el.set_attr("as", as_);
    }
    el
}
