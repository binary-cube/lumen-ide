//! A `.drawio` file: `<mxfile>` holding its pages (`<diagram>`, each an `<mxGraphModel>` whose
//! `<root>` holds the cells), or a bare `<mxGraphModel>` (one page). Read with limits (a
//! hostile file is refused, never followed), each page's ids unique; written back as it was
//! read but for what changed — what's before and after the root, the order of everything,
//! the elements and attributes it doesn't know. A compressed page is read too, and written
//! uncompressed.

use crate::cell::Cell;
use crate::xml::Element;
use crate::{Error, codec};

/// The largest file read (draw.io's own large diagrams are a few megabytes).
pub const MAX_FILE: usize = 64 << 20;
/// The most XML elements in a file (all its pages).
pub const MAX_ELEMENTS: usize = 1_000_000;
/// The most bytes a compressed page may inflate to.
pub const MAX_PAGE: usize = 64 << 20;
/// The most bytes all of a file's compressed pages may inflate to together.
pub const MAX_INFLATED: usize = 256 << 20;

/// What reading a file may still take, shared by all its pages: the elements (the file's
/// own and its compressed pages') and the bytes its pages inflate to.
#[derive(Debug)]
struct Budget {
    elements: usize,
    inflated: usize,
}

/// A diagram file.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// What comes before the root element (an XML declaration), as it was.
    prolog: String,
    /// The `<mxfile>` (its pages' places held by empty `<diagram>`s); `None`: a bare
    /// `<mxGraphModel>`.
    file: Option<Element>,
    pub pages: Vec<Page>,
    /// What comes after the root element (its last line break).
    epilog: String,
}

/// A page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The `<diagram>` (its model's place held by an empty `<mxGraphModel>`).
    diagram: Element,
    /// The `<mxGraphModel>` (its root's place held by an empty `<root>`).
    model: Element,
    /// The `<root>`'s own attributes (its children are the cells).
    root: Element,
    /// The cells, in order (the drawing order: later on top).
    pub cells: Vec<Cell>,
    /// What `<root>` holds that isn't a cell, kept (written after the cells).
    others: Vec<Element>,
}

impl Document {
    /// `text` read.
    pub fn read(text: &str) -> Result<Self, Error> {
        if text.len() > MAX_FILE {
            return Err(Error::TooLarge);
        }
        let mut budget = Budget { elements: 0, inflated: 0 };
        let (prolog, root, epilog) = parse(text, &mut budget)?;
        match root.name.as_str() {
            "mxfile" => {
                let mut file = root;
                let mut pages = Vec::new();
                let mut count = 0;
                for child in &mut file.children {
                    if child.name != "diagram" {
                        continue;
                    }
                    let diagram = std::mem::replace(child, Element::new("diagram"));
                    pages.push(Page::from_diagram(diagram, &mut count, &mut budget)?);
                }
                Ok(Self { prolog, file: Some(file), pages, epilog })
            }
            "mxGraphModel" => {
                let page = Page::from_model(Element::new("diagram"), root, &mut 0)?;
                Ok(Self { prolog, file: None, pages: vec![page], epilog })
            }
            _ => Err(Error::NotDrawio),
        }
    }

    /// A new file: one empty page named `page_name` (`id`: its id).
    pub fn new(page_name: &str, id: &str) -> Self {
        let mut file = Element::new("mxfile");
        file.set_attr("host", "Electron");
        file.children.push(Element::new("diagram"));
        Self {
            prolog: String::new(),
            file: Some(file),
            pages: vec![Page::new(page_name, id)],
            epilog: "\n".into(),
        }
    }

    /// The file's text.
    pub fn write(&self) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str(&self.prolog);
        match &self.file {
            Some(file) => {
                file.write_open(&mut out);
                out.push_str(">\n");
                let mut pages = self.pages.iter();
                for child in &file.children {
                    if child.name == "diagram" {
                        // A page's place (one taken away: its place goes with it).
                        if let Some(page) = pages.next() {
                            page.write(&mut out, 1);
                        }
                    } else {
                        child.write(&mut out, 1);
                    }
                }
                // Pages added since.
                for page in pages {
                    page.write(&mut out, 1);
                }
                file.write_close(&mut out, 0);
                // `write_close` ends with a line break: the epilog says whether the file did.
                if out.ends_with('\n') {
                    out.pop();
                }
            }
            None => {
                if let Some(page) = self.pages.first() {
                    page.write_model(&mut out, 0);
                    out.pop();
                }
            }
        }
        out.push_str(&self.epilog);
        out
    }
}

/// `text` parsed: what's before its root, its root (read whole), what's after. Its elements
/// count against `budget`.
fn parse(text: &str, budget: &mut Budget) -> Result<(String, Element, String), Error> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: u32::try_from(MAX_ELEMENTS * 2).unwrap_or(u32::MAX),
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| Error::Xml(e.to_string()))?;
    let node = doc.root_element();
    let range = node.range();
    let root = Element::read(node, text, &mut budget.elements, MAX_ELEMENTS, 0)?;
    let prolog = text.get(..range.start).unwrap_or_default().to_owned();
    let epilog = text.get(range.end..).unwrap_or_default().to_owned();
    Ok((prolog, root, epilog))
}

impl Page {
    /// What its cells hold in memory, roughly (their fields, styles, points, kept XML).
    pub fn weight(&self) -> u64 {
        self.cells.iter().map(|c| c.weight() as u64).sum()
    }

    /// A new, empty page: its root and its layer (`0`, `1`), as draw.io makes them.
    pub fn new(name: &str, id: &str) -> Self {
        let mut diagram = Element::new("diagram");
        diagram.set_attr("name", name);
        diagram.set_attr("id", id);
        diagram.children.push(Element::new("mxGraphModel"));
        let mut model = Element::new("mxGraphModel");
        for (k, v) in [
            ("grid", "1"),
            ("gridSize", "10"),
            ("guides", "1"),
            ("tooltips", "1"),
            ("connect", "1"),
            ("arrows", "1"),
            ("fold", "1"),
            ("page", "1"),
            ("pageScale", "1"),
            ("pageWidth", "827"),
            ("pageHeight", "1169"),
            ("math", "0"),
            ("shadow", "0"),
        ] {
            model.set_attr(k, v);
        }
        model.children.push(Element::new("root"));
        // The root holds the layers: it has no parent.
        let mut root_cell = Cell::new("0", crate::CellKind::Holder, "");
        root_cell.parent = None;
        let layer = Cell::new("1", crate::CellKind::Holder, "0");
        Self {
            diagram,
            model,
            root: Element::new("root"),
            cells: vec![root_cell, layer],
            others: Vec::new(),
        }
    }

    /// A `<diagram>` read: its model inside it, or compressed in its text.
    fn from_diagram(
        mut diagram: Element,
        count: &mut usize,
        budget: &mut Budget,
    ) -> Result<Self, Error> {
        let at = diagram.children.iter().position(|c| c.name == "mxGraphModel");
        let model = match (at, diagram.text.take()) {
            (Some(i), _) => {
                std::mem::replace(&mut diagram.children[i], Element::new("mxGraphModel"))
            }
            (None, Some(text)) => {
                // What the file's other pages left of the budget.
                let room = MAX_PAGE.min(MAX_INFLATED.saturating_sub(budget.inflated));
                let xml = codec::decompress(&text, room)?;
                budget.inflated = budget.inflated.saturating_add(xml.len());
                let (_, model, _) = parse(&xml, budget)?;
                if model.name != "mxGraphModel" {
                    return Err(Error::NotDrawio);
                }
                diagram.children.push(Element::new("mxGraphModel"));
                model
            }
            // An empty page.
            (None, None) => {
                diagram.children.push(Element::new("mxGraphModel"));
                let mut model = Element::new("mxGraphModel");
                model.children.push(Element::new("root"));
                model
            }
        };
        Self::from_model(diagram, model, count)
    }

    /// A page from its `<diagram>` and its `<mxGraphModel>`; `count`: the cells read so far in
    /// the file.
    fn from_model(diagram: Element, mut model: Element, count: &mut usize) -> Result<Self, Error> {
        let root = match model.children.iter().position(|c| c.name == "root") {
            Some(i) => std::mem::replace(&mut model.children[i], Element::new("root")),
            None => {
                model.children.push(Element::new("root"));
                Element::new("root")
            }
        };
        let mut root = root;
        let mut cells = Vec::with_capacity(root.children.len());
        let mut others = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for el in std::mem::take(&mut root.children) {
            let cell = match Cell::read(el) {
                Ok(cell) => cell,
                Err(el) => {
                    others.push(el);
                    continue;
                }
            };
            *count += 1;
            if *count > crate::MAX_CELLS {
                return Err(Error::TooManyCells);
            }
            if !ids.insert(cell.id.clone()) {
                return Err(Error::DuplicateId(cell.id));
            }
            cells.push(cell);
        }
        Ok(Self { diagram, model, root, cells, others })
    }

    /// Its id (`<diagram id>`).
    pub fn id(&self) -> Option<&str> {
        self.diagram.attr("id")
    }

    /// Its name (the tab's).
    pub fn name(&self) -> &str {
        self.diagram.attr("name").unwrap_or_default()
    }

    pub fn set_name(&mut self, name: &str) {
        self.diagram.set_attr("name", name);
    }

    /// An attribute of its model (`grid`, `gridSize`, `background`, `pageWidth`…).
    pub fn setting(&self, name: &str) -> Option<&str> {
        self.model.attr(name)
    }

    pub fn set_setting(&mut self, name: &str, value: &str) {
        self.model.set_attr(name, value);
    }

    pub fn remove_setting(&mut self, name: &str) {
        self.model.remove_attr(name);
    }

    /// The grid shown.
    pub fn grid(&self) -> bool {
        self.setting("grid") != Some("0")
    }

    /// The grid's step (draw.io's default: 10).
    pub fn grid_size(&self) -> f64 {
        self.setting("gridSize")
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| v.is_finite() && *v > 0.0)
            .unwrap_or(10.0)
    }

    /// The page's size (draw.io's default: A4 portrait, 827 × 1169).
    pub fn page_size(&self) -> (f64, f64) {
        let get = |name, default| {
            self.setting(name)
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| v.is_finite() && *v > 0.0)
                .unwrap_or(default)
        };
        (get("pageWidth", 827.0), get("pageHeight", 1169.0))
    }

    /// The background's color (`None`: none, the sheet's).
    pub fn background(&self) -> Option<&str> {
        self.setting("background").filter(|v| !v.is_empty() && *v != "none")
    }

    /// The cell `id`.
    pub fn cell(&self, id: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }

    pub fn cell_mut(&mut self, id: &str) -> Option<&mut Cell> {
        self.cells.iter_mut().find(|c| c.id == id)
    }

    /// Written at `depth`: its `<diagram>` holding its model (never compressed).
    fn write(&self, out: &mut String, depth: usize) {
        for _ in 0..depth {
            out.push_str("  ");
        }
        self.diagram.write_open(out);
        out.push_str(">\n");
        let mut model_written = false;
        for child in &self.diagram.children {
            if child.name == "mxGraphModel" && !model_written {
                self.write_model(out, depth + 1);
                model_written = true;
            } else {
                child.write(out, depth + 1);
            }
        }
        if !model_written {
            self.write_model(out, depth + 1);
        }
        self.diagram.write_close(out, depth);
    }

    /// Its `<mxGraphModel>` alone (the clipboard's XML).
    pub(crate) fn model_xml(&self) -> String {
        let mut out = String::new();
        self.write_model(&mut out, 0);
        out
    }

    /// Its `<mxGraphModel>` at `depth`.
    fn write_model(&self, out: &mut String, depth: usize) {
        for _ in 0..depth {
            out.push_str("  ");
        }
        self.model.write_open(out);
        out.push_str(">\n");
        for child in &self.model.children {
            if child.name == "root" {
                self.write_root(out, depth + 1);
            } else {
                child.write(out, depth + 1);
            }
        }
        self.model.write_close(out, depth);
    }

    fn write_root(&self, out: &mut String, depth: usize) {
        for _ in 0..depth {
            out.push_str("  ");
        }
        self.root.write_open(out);
        out.push_str(">\n");
        for cell in &self.cells {
            cell.element().write(out, depth + 1);
        }
        for other in &self.others {
            other.write(out, depth + 1);
        }
        self.root.write_close(out, depth);
    }
}

#[cfg(test)]
#[path = "../tests/unit/document.rs"]
mod tests;
