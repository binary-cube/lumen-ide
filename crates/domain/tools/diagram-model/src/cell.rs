//! A cell of a page (`<mxCell>`): the root, a layer, a shape (vertex) or an edge — or one
//! wrapped in `<UserObject>` / `<object>`, whose own properties (and its label, its id) are
//! on the wrapper. Read into typed fields; the element it came from is kept, so writing it
//! touches only what changed and keeps whatever else it holds.

use crate::geometry::Geometry;
use crate::style::Style;
use crate::xml::Element;

/// What a cell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// Drawn as a shape (a group too: a shape holding others).
    Vertex,
    /// A line between two cells (or points).
    Edge,
    /// The page's root (id `0`) or a layer (the root's children): what holds the others.
    Holder,
}

/// The fields a cell is read into.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub id: String,
    pub parent: Option<String>,
    pub kind: CellKind,
    /// Its text (draw.io's HTML when the style says `html=1`).
    pub value: String,
    pub style: Style,
    pub source: Option<String>,
    pub target: Option<String>,
    pub geometry: Option<Geometry>,
    /// Hidden (`visible="0"`).
    pub visible: bool,
    /// The element it was read from (a new cell: one made for it).
    xml: Element,
    /// Read from the file (`None`: a new cell), to write only what changed: its geometry as
    /// read — its other fields are compared with the element kept (never changed), not kept
    /// twice.
    read: Option<Box<Read>>,
}

/// What's kept of a cell as read besides its element.
#[derive(Debug, Clone, PartialEq)]
struct Read {
    geometry: Option<Geometry>,
}

/// The element holding a cell's own attributes: the `mxCell` itself, or the one a wrapper
/// holds.
fn inner(el: &Element) -> Option<&Element> {
    if el.name == "mxCell" { Some(el) } else { el.child("mxCell", None) }
}

fn inner_mut(el: &mut Element) -> Option<&mut Element> {
    if el.name == "mxCell" { Some(el) } else { el.child_mut("mxCell", None) }
}

impl Cell {
    /// `el` read (an `mxCell`, or a wrapper holding one); not a cell: `el` given back.
    pub(crate) fn read(el: Element) -> Result<Self, Element> {
        let (Some(cell), Some(id)) = (inner(&el), el.attr("id")) else { return Err(el) };
        let wrapped = el.name != "mxCell";
        let id = id.to_owned();
        let value = if wrapped { el.attr("label") } else { el.attr("value") };
        let kind = if cell.attr("edge") == Some("1") {
            CellKind::Edge
        } else if cell.attr("vertex") == Some("1") {
            CellKind::Vertex
        } else {
            CellKind::Holder
        };
        let geometry = cell.child("mxGeometry", Some("geometry")).map(Geometry::read);
        let fields = Self {
            id,
            parent: cell.attr("parent").map(str::to_owned),
            kind,
            value: value.unwrap_or_default().to_owned(),
            style: Style::parse(cell.attr("style").unwrap_or_default()),
            source: cell.attr("source").map(str::to_owned),
            target: cell.attr("target").map(str::to_owned),
            geometry: geometry.clone(),
            visible: cell.attr("visible") != Some("0"),
            xml: Element::new("mxCell"),
            read: Some(Box::new(Read { geometry })),
        };
        Ok(Self { xml: el, ..fields })
    }

    /// A new cell (a shape or an edge) in `parent`.
    pub fn new(id: impl Into<String>, kind: CellKind, parent: impl Into<String>) -> Self {
        let mut xml = Element::new("mxCell");
        let id = id.into();
        xml.set_attr("id", id.clone());
        Self {
            id,
            parent: Some(parent.into()),
            kind,
            value: String::new(),
            style: Style::default(),
            source: None,
            target: None,
            geometry: (kind != CellKind::Holder).then(Geometry::default),
            visible: true,
            xml,
            read: None,
        }
    }

    /// Wrapped in `<UserObject>` / `<object>` (its properties on the wrapper).
    pub fn is_wrapped(&self) -> bool {
        self.xml.name != "mxCell"
    }

    /// The wrapper's own properties (a `UserObject`'s `link`, `tooltip`, anything the user
    /// added), in order; none for a plain cell.
    pub fn properties(&self) -> impl Iterator<Item = (&str, &str)> {
        let own = self.is_wrapped();
        self.xml.attrs().filter(move |(k, _)| own && !matches!(*k, "id" | "label" | "placeholders"))
    }

    /// Attribute `name` of its own element as read.
    fn read_attr(&self, name: &str) -> Option<&str> {
        inner(&self.xml).and_then(|c| c.attr(name))
    }

    /// Its text as read (on the wrapper when there is one).
    fn read_value(&self) -> &str {
        let label = if self.is_wrapped() { "label" } else { "value" };
        self.xml.attr(label).unwrap_or_default()
    }

    fn id_changed(&self) -> bool {
        self.xml.attr("id") != Some(self.id.as_str())
    }

    fn style_changed(&self) -> bool {
        !self.style.reads_as(self.read_attr("style").unwrap_or_default())
    }

    fn visible_changed(&self) -> bool {
        (self.read_attr("visible") != Some("0")) != self.visible
    }

    /// Changed since read (a new cell: always).
    pub fn is_changed(&self) -> bool {
        self.read.as_deref().is_none_or(|r| {
            self.id_changed()
                || self.read_attr("parent") != self.parent.as_deref()
                || self.read_value() != self.value
                || self.style_changed()
                || self.read_attr("source") != self.source.as_deref()
                || self.read_attr("target") != self.target.as_deref()
                || r.geometry != self.geometry
                || self.visible_changed()
        })
    }

    /// Its element, with what changed written into it.
    /// What it holds in memory, roughly (its fields, its points, the XML kept with it).
    pub(crate) fn weight(&self) -> usize {
        let points = self.geometry.as_ref().map_or(0, |g| g.points.len() * 16);
        256 + self.value.len() + self.id.len() + self.style.weight() + points + self.xml.weight()
    }

    pub(crate) fn element(&self) -> std::borrow::Cow<'_, Element> {
        if !self.is_changed() {
            return std::borrow::Cow::Borrowed(&self.xml);
        }
        let mut xml = self.xml.clone();
        let read = self.read.as_deref();
        let wrapped = self.is_wrapped();
        // The id and the label: on the wrapper when there is one.
        if read.is_none() || self.id_changed() {
            xml.set_attr("id", self.id.clone());
        }
        let label = if wrapped { "label" } else { "value" };
        if read.is_some() && self.read_value() != self.value {
            xml.set_attr(label, self.value.clone());
        }
        // What it was read as (asked before its element changes below).
        let (style_changed, visible_changed) = (self.style_changed(), self.visible_changed());
        let ends = [
            ("parent", self.parent.as_deref()),
            ("source", self.source.as_deref()),
            ("target", self.target.as_deref()),
        ]
        .map(|(name, now)| (name, now, read.map(|_| self.read_attr(name))));
        let Some(cell) = inner_mut(&mut xml) else { return std::borrow::Cow::Owned(xml) };
        if read.is_none() && self.kind != CellKind::Holder {
            // A new shape's or edge's attributes in draw.io's order (a holder has none:
            // `<mxCell id="0" />`).
            cell.set_attr("value", self.value.clone());
            cell.set_attr("style", self.style.to_string());
            match self.kind {
                CellKind::Vertex => cell.set_attr("vertex", "1"),
                _ => cell.set_attr("edge", "1"),
            }
        } else if read.is_some() && style_changed {
            cell.set_attr("style", self.style.to_string());
        }
        for (name, now, before) in ends {
            set_optional(cell, name, now, before);
        }
        if read.is_none() || visible_changed {
            if self.visible {
                cell.remove_attr("visible");
            } else {
                cell.set_attr("visible", "0");
            }
        }
        let before = read.and_then(|r| r.geometry.as_ref());
        match &self.geometry {
            Some(g) if read.is_none_or(|r| r.geometry.as_ref() != Some(g)) => {
                match cell.child_mut("mxGeometry", Some("geometry")) {
                    Some(el) => g.write(el, before),
                    None => {
                        let mut el = Element::new("mxGeometry");
                        g.write(&mut el, None);
                        el.set_attr("as", "geometry");
                        cell.children.push(el);
                    }
                }
            }
            None => cell.children.retain(|c| !(c.name == "mxGeometry")),
            Some(_) => {}
        }
        std::borrow::Cow::Owned(xml)
    }
}

/// Optional attribute `name` set (or taken away) when it changed from `before` (`None`:
/// a new cell).
fn set_optional(el: &mut Element, name: &str, now: Option<&str>, before: Option<Option<&str>>) {
    if before == Some(now) {
        return;
    }
    match now {
        Some(v) => el.set_attr(name, v),
        None => el.remove_attr(name),
    }
}
