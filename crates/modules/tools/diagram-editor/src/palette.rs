//! The shapes palette's catalog — its sections in the order chosen (General, then Flowchart,
//! Entity Relation, UML, Arrows), each shape with draw.io's own style, size and text — and
//! its thumbnails: an open section's shapes rasterized on a worker, once per theme and scale,
//! taken at the next poll (made here and now without a waker: tests, snapshots).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use diagram_model::Style;
use kernel::{Point, Rect};
use vector_tiles::Mask;

use crate::drawing::{Item, Theme, shape};

/// A shape of the palette: its id (its title's key), draw.io's style, its size, its text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub id: &'static str,
    pub style: &'static str,
    pub size: (f32, f32),
    pub value: &'static str,
}

/// A section of the palette.
#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub id: &'static str,
    pub entries: &'static [Entry],
}

const fn entry(
    id: &'static str,
    style: &'static str,
    size: (f32, f32),
    value: &'static str,
) -> Entry {
    Entry { id, style, size, value }
}

/// The palette, in order.
pub const SECTIONS: &[Section] = &[
    Section {
        id: "general",
        entries: &[
            entry("rectangle", "rounded=0;whiteSpace=wrap;html=1;", (120.0, 60.0), ""),
            entry("rounded", "rounded=1;whiteSpace=wrap;html=1;", (120.0, 60.0), ""),
            entry(
                "text",
                "text;html=1;align=center;verticalAlign=middle;whiteSpace=wrap;",
                (60.0, 30.0),
                "Text",
            ),
            entry("ellipse", "ellipse;whiteSpace=wrap;html=1;", (120.0, 80.0), ""),
            entry("square", "whiteSpace=wrap;html=1;aspect=fixed;", (80.0, 80.0), ""),
            entry("circle", "ellipse;whiteSpace=wrap;html=1;aspect=fixed;", (80.0, 80.0), ""),
            entry(
                "process",
                "shape=process;whiteSpace=wrap;html=1;backgroundOutline=1;",
                (120.0, 60.0),
                "",
            ),
            entry("diamond", "rhombus;whiteSpace=wrap;html=1;", (80.0, 80.0), ""),
            entry(
                "parallelogram",
                "shape=parallelogram;perimeter=parallelogramPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
                (120.0, 60.0),
                "",
            ),
            entry(
                "hexagon",
                "shape=hexagon;perimeter=hexagonPerimeter2;whiteSpace=wrap;html=1;fixedSize=1;",
                (120.0, 80.0),
                "",
            ),
            entry("triangle", "triangle;whiteSpace=wrap;html=1;", (60.0, 80.0), ""),
            entry(
                "cylinder",
                "shape=cylinder3;whiteSpace=wrap;html=1;boundedLbl=1;backgroundOutline=1;size=15;",
                (60.0, 80.0),
                "",
            ),
            entry("cloud", "ellipse;shape=cloud;whiteSpace=wrap;html=1;", (120.0, 80.0), ""),
            entry(
                "document",
                "shape=document;whiteSpace=wrap;html=1;boundedLbl=1;",
                (120.0, 80.0),
                "",
            ),
            entry(
                "note",
                "shape=note;whiteSpace=wrap;html=1;backgroundOutline=1;darkOpacity=0.05;",
                (80.0, 100.0),
                "",
            ),
            entry("actor", "shape=actor;whiteSpace=wrap;html=1;", (40.0, 60.0), ""),
        ],
    },
    Section {
        id: "basic",
        entries: &[
            entry(
                "trapezoid",
                "shape=trapezoid;perimeter=trapezoidPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
                (120.0, 60.0),
                "",
            ),
            entry(
                "step",
                "shape=step;perimeter=stepPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
                (120.0, 80.0),
                "",
            ),
            entry("card", "shape=card;whiteSpace=wrap;html=1;", (80.0, 100.0), ""),
            entry(
                "callout",
                "shape=callout;whiteSpace=wrap;html=1;perimeter=calloutPerimeter;",
                (120.0, 80.0),
                "",
            ),
            entry("tape", "shape=tape;whiteSpace=wrap;html=1;", (120.0, 100.0), ""),
            entry("delay", "shape=delay;whiteSpace=wrap;html=1;", (80.0, 40.0), ""),
            entry("off_page", "shape=offPageConnector;whiteSpace=wrap;html=1;", (80.0, 80.0), ""),
            entry("manual_input", "shape=manualInput;whiteSpace=wrap;html=1;", (80.0, 80.0), ""),
        ],
    },
    Section {
        id: "flowchart",
        entries: &[
            entry(
                "terminator",
                "rounded=1;whiteSpace=wrap;html=1;arcSize=50;",
                (120.0, 40.0),
                "Start",
            ),
            entry("process", "rounded=0;whiteSpace=wrap;html=1;", (120.0, 60.0), "Process"),
            entry("decision", "rhombus;whiteSpace=wrap;html=1;", (100.0, 80.0), "Decision"),
            entry(
                "data",
                "shape=parallelogram;perimeter=parallelogramPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
                (120.0, 60.0),
                "Data",
            ),
            entry(
                "document",
                "shape=document;whiteSpace=wrap;html=1;boundedLbl=1;",
                (120.0, 80.0),
                "Document",
            ),
            entry(
                "subprocess",
                "shape=process;whiteSpace=wrap;html=1;backgroundOutline=1;",
                (120.0, 60.0),
                "Subprocess",
            ),
            entry(
                "database",
                "shape=cylinder3;whiteSpace=wrap;html=1;boundedLbl=1;backgroundOutline=1;size=15;",
                (60.0, 80.0),
                "Database",
            ),
        ],
    },
    Section {
        id: "entity_relation",
        entries: &[
            entry(
                "entity",
                "swimlane;fontStyle=0;childLayout=stackLayout;horizontal=1;startSize=26;horizontalStack=0;resizeParent=1;resizeParentMax=0;resizeLast=0;collapsible=1;marginBottom=0;html=1;",
                (140.0, 110.0),
                "Entity",
            ),
            entry(
                "attribute",
                "ellipse;whiteSpace=wrap;html=1;align=center;",
                (100.0, 40.0),
                "Attribute",
            ),
            entry(
                "relationship",
                "shape=rhombus;perimeter=rhombusPerimeter;whiteSpace=wrap;html=1;align=center;",
                (120.0, 60.0),
                "Relationship",
            ),
        ],
    },
    Section {
        id: "uml",
        entries: &[
            entry(
                "class",
                "swimlane;fontStyle=1;align=center;verticalAlign=top;childLayout=stackLayout;horizontal=1;startSize=26;horizontalStack=0;resizeParent=1;resizeParentMax=0;resizeLast=0;collapsible=1;marginBottom=0;html=1;",
                (160.0, 90.0),
                "Class",
            ),
            entry(
                "note",
                "shape=note;whiteSpace=wrap;html=1;size=14;verticalAlign=top;align=left;spacingTop=-6;",
                (100.0, 70.0),
                "Note",
            ),
            entry("actor", "shape=actor;whiteSpace=wrap;html=1;", (40.0, 60.0), ""),
            entry(
                "component",
                "shape=process;whiteSpace=wrap;html=1;backgroundOutline=1;size=0.08;",
                (140.0, 60.0),
                "Component",
            ),
        ],
    },
    Section {
        id: "arrows",
        entries: &[
            entry("arrow", "shape=singleArrow;whiteSpace=wrap;html=1;", (100.0, 60.0), ""),
            entry("double_arrow", "shape=doubleArrow;whiteSpace=wrap;html=1;", (100.0, 60.0), ""),
        ],
    },
];

/// Every shape whose title holds `query` (case aside), with its section.
pub fn search(query: &str, title: impl Fn(&Section, &Entry) -> String) -> Vec<(usize, usize)> {
    let query = query.trim().to_lowercase();
    let mut out = Vec::new();
    for (s, section) in SECTIONS.iter().enumerate() {
        for (e, entry) in section.entries.iter().enumerate() {
            if query.is_empty()
                || title(section, entry).to_lowercase().contains(&query)
                || entry.id.contains(&query)
            {
                out.push((s, e));
            }
        }
    }
    out
}

/// A thumbnail's box (logical pixels).
pub const THUMB: (f32, f32) = (40.0, 32.0);

/// `entry` drawn in a thumbnail's box at scale `k`: its masks. Drawn at its own size (its
/// proportions in pixels — a cylinder's rim, a note's fold — as they are), then made small.
pub fn thumbnail(entry: &Entry, theme: &Theme, k: f32) -> Vec<Mask> {
    let (w, h) = entry.size;
    // Fitted in the box (a margin around), its proportions kept.
    let fit = ((THUMB.0 - 4.0) / w).min((THUMB.1 - 4.0) / h);
    let (dx, dy) = ((THUMB.0 - w * fit) / 2.0, (THUMB.1 - h * fit) / 2.0);
    let r = Rect::new(0.0, 0.0, w, h);
    let style = Style::parse(entry.style);
    let mut look = crate::look::look(&style, w, h);
    // A hairline in the small box, whatever its own width.
    look.stroke_width = 1.0;
    let mut items: Vec<Item> = Vec::new();
    if look.shape.is_none() {
        // A text-only shape: its place, dashed.
        items.push(Item {
            segs: vector_tiles::rounded(r, 2.0 / fit),
            fill: None,
            line: Some(vector_tiles::Line::dashed(theme.stroke, 1.0)),
        });
    } else {
        shape(r, &look, theme, &mut items);
    }
    let small = |p: &mut Point| {
        p.x = p.x * fit + dx;
        p.y = p.y * fit + dy;
    };
    for item in &mut items {
        for seg in &mut item.segs {
            match seg {
                vector_tiles::Seg::Move(p) | vector_tiles::Seg::Line(p) => small(p),
                vector_tiles::Seg::Cubic(a, b, p) => {
                    small(a);
                    small(b);
                    small(p);
                }
                vector_tiles::Seg::Close => {}
            }
        }
    }
    let shapes = items.iter().map(|i| vector_tiles::Shape {
        segs: &i.segs,
        fill: i.fill,
        line: i.line.as_ref(),
    });
    vector_tiles::tile(THUMB, shapes, k, 0, 0)
}

type Waker = Arc<dyn Fn() + Send + Sync>;

/// A section's thumbnails, for (theme, scale).
/// What the thumbnails are made for: the theme's colors, the scale.
type MadeFor = ([[u8; 4]; 4], u32);
/// A section's thumbnails, and what they were made for.
type Made = (usize, MadeFor, Vec<Arc<Vec<Mask>>>);

/// The thumbnails made: per section, for one theme and scale (another: made again).
#[derive(Default)]
pub struct Thumbnails {
    made: HashMap<usize, Vec<Arc<Vec<Mask>>>>,
    asked: std::collections::HashSet<usize>,
    /// What they were made for.
    made_for: Option<MadeFor>,
    done: Option<(Sender<Made>, Receiver<Made>)>,
}

impl std::fmt::Debug for Thumbnails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thumbnails").field("sections", &self.made.len()).finish_non_exhaustive()
    }
}

impl Thumbnails {
    /// Section `s`'s thumbnails for `theme` at scale `k`; asked for (on the worker, or here
    /// without a waker) when missing.
    pub fn section(
        &mut self,
        s: usize,
        theme: &Theme,
        k: f32,
        waker: Option<&Waker>,
    ) -> Option<&[Arc<Vec<Mask>>]> {
        let key = (
            [theme.canvas, theme.fill, theme.stroke, theme.text].map(kernel::Rgba::to_rgba8),
            k.to_bits(),
        );
        if self.made_for != Some(key) {
            self.made.clear();
            self.asked.clear();
            self.made_for = Some(key);
        }
        // A texture the renderer freed (the palette hidden long enough): made again.
        let lost = |t: &[Arc<Vec<Mask>>]| t.iter().any(|m| m.iter().any(|(i, _)| i.is_lost()));
        if self.made.get(&s).is_some_and(|t| lost(t)) {
            self.made.remove(&s);
        }
        if !self.made.contains_key(&s) && !self.asked.contains(&s) {
            let section = *SECTIONS.get(s)?;
            let theme = *theme;
            let make = move || {
                section
                    .entries
                    .iter()
                    .map(|e| Arc::new(thumbnail(e, &theme, k)))
                    .collect::<Vec<_>>()
            };
            match waker {
                None => {
                    self.made.insert(s, make());
                }
                Some(waker) => {
                    self.asked.insert(s);
                    let tx = self.done.get_or_insert_with(channel).0.clone();
                    let waker = Arc::clone(waker);
                    kernel::work::run("diagram-palette", move || {
                        if tx.send((s, key, make())).is_ok() {
                            waker();
                        }
                    });
                }
            }
        }
        self.made.get(&s).map(Vec::as_slice)
    }

    /// The sections made since, taken. `true`: something new.
    pub fn poll(&mut self) -> bool {
        let Some((_, rx)) = &self.done else { return false };
        let mut any = false;
        while let Ok((s, made_for, thumbs)) = rx.try_recv() {
            // Made for another theme or scale (changed since it was asked): let go.
            if Some(made_for) != self.made_for {
                continue;
            }
            self.asked.remove(&s);
            self.made.insert(s, thumbs);
            any = true;
        }
        any
    }

    /// What they hold (bytes).
    /// Everything let go (made again when shown); what's being made is dropped when it comes.
    pub fn clear(&mut self) {
        self.made.clear();
        self.asked.clear();
        self.made_for = None;
    }

    pub fn bytes(&self) -> u64 {
        self.made
            .values()
            .flatten()
            .flat_map(|m| m.iter())
            .map(|(i, _)| u64::from(i.width) * u64::from(i.height) * i.bytes_per_pixel())
            .sum()
    }
}

/// Where a thumbnail's masks go: its box's corner on the screen (the masks are in physical
/// pixels from it).
pub fn place(corner: Point, r: Rect) -> Rect {
    Rect::new(corner.x + r.min_x(), corner.y + r.min_y(), r.size.width, r.size.height)
}

#[cfg(test)]
#[path = "../tests/unit/palette.rs"]
mod tests;
