//! A page's accessibility tree, as Chromium tells it (while the app asks: a screen reader
//! runs), kept here from its updates and handed to the app whole each time it changes: the
//! nodes a screen reader reads (a role, a name, a value, a state, a place in the page),
//! without Chromium's own scaffolding (ignored nodes, text boxes, unnamed containers), so
//! the app puts them under the page's node in its window's tree.
//!
//! Chromium's updates are read as JSON (CEF hands them as values, turned once by
//! [`to_json`]); a page is hostile: the nodes kept, the names and the depth are bounded.

use std::collections::{HashMap, HashSet};

use cef::*;
use serde_json::Value as Json;
use web_engine_protocol::AccessNode;

/// The most nodes told for a page (a huge document: the first ones, in order).
pub const MAX_NODES: usize = 4000;
/// The most nodes kept from Chromium's updates (what's beyond is dropped).
const MAX_KEPT: usize = 50_000;
/// The longest name or value told (characters).
const MAX_TEXT: usize = 1000;
/// The deepest a node is followed (containers inside containers).
const MAX_DEPTH: usize = 256;

/// A node as Chromium last told it.
#[derive(Debug, Clone, Default)]
struct Raw {
    role: String,
    name: String,
    value: Option<String>,
    children: Vec<i64>,
    /// The node its `rect` is relative to (0: the page).
    container: i64,
    rect: [f32; 4],
    /// How far it's scrolled (its children's places move by it).
    scroll: (f32, f32),
    /// What its `rect` is drawn through (a CSS or SVG transform: scaled, moved), its 2D
    /// part; `None`: as it is.
    transform: Option<Affine>,
    ignored: bool,
    disabled: bool,
    selected: bool,
    checked: Option<bool>,
    expanded: Option<bool>,
    level: Option<u8>,
}

/// A node on the walk of [`Tree::told`]: its id, the index in what's told of the kept node
/// it goes under, its depth.
type Walked = (i64, Option<usize>, usize);

/// A page's tree, from Chromium's updates.
#[derive(Debug, Default)]
pub struct Tree {
    root: Option<i64>,
    focus: Option<i64>,
    nodes: HashMap<i64, Raw>,
    /// The walks' own lists, kept between them (cleared, never made again per update or tree
    /// told): the nodes seen, the ones still to walk.
    seen: HashSet<i64>,
    walking: Vec<i64>,
    stack: Vec<Walked>,
}

impl Tree {
    /// An update applied (Chromium's `on_accessibility_tree_change`, as JSON): its nodes
    /// replace the ones of their ids, what it clears goes, its tree data says the root and
    /// the focus. What's no longer reachable goes too, looked for only when the tree's shape
    /// changed (a node's children, a node new, one cleared, another root): most updates only
    /// change a name, a value or a state.
    pub fn apply(&mut self, change: &Json) {
        let mut reshaped = false;
        for update in change.get("updates").and_then(Json::as_array).into_iter().flatten() {
            if let Some(cleared) = update.get("node_id_to_clear").and_then(Json::as_i64)
                && cleared != 0
            {
                self.clear_below(cleared);
                reshaped = true;
            }
            if let Some(root) = update.get("root_id").and_then(Json::as_i64) {
                reshaped |= self.root != Some(root);
                self.root = Some(root);
            }
            if let Some(focus) = update.pointer("/tree_data/focus_id").and_then(Json::as_i64) {
                self.focus = Some(focus);
            }
            for node in update.get("nodes").and_then(Json::as_array).into_iter().flatten() {
                let Some(id) = node.get("id").and_then(Json::as_i64) else { continue };
                if self.nodes.len() >= MAX_KEPT && !self.nodes.contains_key(&id) {
                    continue;
                }
                let node = raw(node);
                let before = self.nodes.get(&id).map(|old| old.children.as_slice());
                reshaped |= before != Some(node.children.as_slice());
                self.nodes.insert(id, node);
            }
        }
        if reshaped {
            self.prune();
        }
    }

    /// Nodes moved by the layout alone (Chromium's `on_accessibility_location_change`, as
    /// JSON: their new places, the containers they're relative to).
    pub fn moved(&mut self, moves: &Json) {
        for moved in moves.as_array().into_iter().flatten() {
            let Some(id) = moved.get("id").and_then(Json::as_i64) else { continue };
            let Some(node) = self.nodes.get_mut(&id) else { continue };
            let Some(location) = moved.get("new_location") else { continue };
            if let Some(bounds) = location.get("bounds") {
                node.rect = rect_of(bounds);
            }
            if let Some(container) = location.get("offset_container_id").and_then(Json::as_i64) {
                node.container = container;
            }
            // A location tells the node's whole place: without a transform, it has none now.
            node.transform =
                location.get("transform").and_then(Json::as_str).and_then(Affine::parse);
        }
    }

    /// The nodes a screen reader reads, parents first, places in the page; at most
    /// [`MAX_NODES`].
    pub fn told(&mut self) -> Vec<AccessNode> {
        let mut told = Vec::new();
        let Some(root) = self.root else { return told };
        let mut stack = std::mem::take(&mut self.stack);
        let mut seen = std::mem::take(&mut self.seen);
        stack.clear();
        seen.clear();
        stack.push((root, None, 0));
        while let Some((id, parent, depth)) = stack.pop() {
            if told.len() >= MAX_NODES {
                break;
            }
            // A node is walked once (a hostile tree may loop).
            if depth > MAX_DEPTH || !seen.insert(id) {
                continue;
            }
            let Some(node) = self.nodes.get(&id) else { continue };
            let under = parent.and_then(|i| told.get(i));
            let kept = id != root && self.kept(node, under.map(|n| n.name.as_str()));
            let next = if kept { Some(told.len()) } else { parent };
            if kept {
                told.push(AccessNode {
                    id,
                    parent: under.map(|n| n.id),
                    role: node.role.clone(),
                    name: node.name.clone(),
                    value: node.value.clone(),
                    rect: self.rect(id),
                    focused: self.focus == Some(id),
                    disabled: node.disabled,
                    selected: node.selected,
                    checked: node.checked,
                    expanded: node.expanded,
                    level: node.level,
                });
            }
            // Reversed: the first child comes off the stack first.
            for child in node.children.iter().rev() {
                stack.push((*child, next, depth + 1));
            }
        }
        (self.stack, self.seen) = (stack, seen);
        told
    }

    /// Whether `node` is told: not Chromium's scaffolding, nor a text saying again the name
    /// of what it's in (a button's label).
    fn kept(&self, node: &Raw, under: Option<&str>) -> bool {
        if node.ignored {
            return false;
        }
        match node.role.as_str() {
            "inlineTextBox" | "none" | "presentational" | "lineBreak" => false,
            "genericContainer" | "group" | "section" => !node.name.is_empty(),
            "staticText" => !node.name.trim().is_empty() && under != Some(node.name.as_str()),
            _ => true,
        }
    }

    /// Node `id`'s place in the page, as Chromium places it: its own transform, then its
    /// container's place (less its scroll), up to the page.
    fn rect(&self, id: i64) -> [f32; 4] {
        let Some(mut node) = self.nodes.get(&id) else { return [0.0; 4] };
        let mut rect = node.rect;
        for _ in 0..MAX_DEPTH {
            if let Some(transform) = &node.transform {
                rect = transform.bounds(rect);
            }
            let container = node.container;
            let Some(outer) = self.nodes.get(&container).filter(|_| container != 0) else { break };
            rect[0] += outer.rect[0] - outer.scroll.0;
            rect[1] += outer.rect[1] - outer.scroll.1;
            node = outer;
        }
        rect
    }

    /// What's under `id` goes (it's kept: its update follows).
    fn clear_below(&mut self, id: i64) {
        let mut going: Vec<i64> =
            self.nodes.get(&id).map(|n| n.children.clone()).unwrap_or_default();
        while let Some(child) = going.pop() {
            if let Some(node) = self.nodes.remove(&child) {
                going.extend(node.children);
            }
        }
    }

    /// Nodes no longer reachable from the root go.
    fn prune(&mut self) {
        let Some(root) = self.root else {
            self.nodes.clear();
            return;
        };
        self.seen.clear();
        self.walking.clear();
        self.walking.push(root);
        while let Some(id) = self.walking.pop() {
            if self.seen.insert(id)
                && let Some(node) = self.nodes.get(&id)
            {
                self.walking.extend(&node.children);
            }
        }
        self.nodes.retain(|id, _| self.seen.contains(id));
    }
}

/// A node of an update as kept.
fn raw(node: &Json) -> Raw {
    let attributes = node.get("attributes");
    let text = |key: &str| {
        let text = attributes?.get(key)?.as_str()?;
        Some(text.chars().take(MAX_TEXT).collect::<String>())
    };
    let number = |key: &str| attributes.and_then(|a| a.get(key)).and_then(Json::as_f64);
    let states = node.get("state").and_then(Json::as_array).map_or(&[][..], Vec::as_slice);
    let has = |state: &str| states.iter().any(|told| told.as_str() == Some(state));
    let expanded = match (has("expanded"), has("collapsed")) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    };
    let checked = match text("checkedState").as_deref() {
        Some("true" | "mixed") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    Raw {
        role: node.get("role").and_then(Json::as_str).unwrap_or_default().to_owned(),
        name: text("name").unwrap_or_default(),
        value: text("value").filter(|v| !v.is_empty()),
        children: node
            .get("child_ids")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_i64)
            // No node has more children than nodes are kept (a hostile one names millions).
            .take(MAX_KEPT)
            .collect(),
        container: node.get("offset_container_id").and_then(Json::as_i64).unwrap_or(0),
        rect: node.get("location").map_or([0.0; 4], rect_of),
        transform: node.get("transform").and_then(Json::as_str).and_then(Affine::parse),
        scroll: (number("scrollX").unwrap_or(0.0) as f32, number("scrollY").unwrap_or(0.0) as f32),
        ignored: has("ignored") || has("invisible"),
        disabled: text("restriction").as_deref() == Some("disabled"),
        selected: attributes.and_then(|a| a.get("selected")).and_then(Json::as_bool) == Some(true),
        checked,
        expanded,
        level: number("hierarchicalLevel").and_then(|l| u8::try_from(l as i64).ok()),
    }
}

/// A transform's 2D part: `x' = a·x + b·y + tx`, `y' = c·x + d·y + ty`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Affine {
    a: f32,
    b: f32,
    tx: f32,
    c: f32,
    d: f32,
    ty: f32,
}

impl Affine {
    /// Chromium's text of a 4×4 matrix, row by row (`[ 2 0 0 50\n 0 2 0 0\n …]`); `None`:
    /// not one, or one that isn't finite.
    fn parse(text: &str) -> Option<Self> {
        let numbers: Vec<f32> = text
            .split(|c: char| c.is_whitespace() || c == '[' || c == ']')
            .filter(|part| !part.is_empty())
            .take(MATRIX_VALUES + 1)
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if numbers.len() != MATRIX_VALUES || numbers.iter().any(|n| !n.is_finite()) {
            return None;
        }
        let m = |row: usize, column: usize| numbers[row * 4 + column];
        Some(Self { a: m(0, 0), b: m(0, 1), tx: m(0, 3), c: m(1, 0), d: m(1, 1), ty: m(1, 3) })
    }

    /// The box `rect` (x, y, width, height) covers once drawn through it.
    fn bounds(&self, [x, y, width, height]: [f32; 4]) -> [f32; 4] {
        let corners = [(x, y), (x + width, y), (x, y + height), (x + width, y + height)];
        let placed = corners.map(|(px, py)| {
            (self.a * px + self.b * py + self.tx, self.c * px + self.d * py + self.ty)
        });
        let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
        let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (px, py) in placed {
            (left, top, right, bottom) = (left.min(px), top.min(py), right.max(px), bottom.max(py));
        }
        [left, top, right - left, bottom - top]
    }
}

/// The values of a 4×4 matrix.
const MATRIX_VALUES: usize = 16;

/// A place as Chromium writes it (`x`, `y`, `width`, `height`; what's missing is 0).
fn rect_of(place: &Json) -> [f32; 4] {
    let at = |key: &str| place.get(key).and_then(Json::as_f64).unwrap_or(0.0) as f32;
    [at("x"), at("y"), at("width"), at("height")]
}

/// A CEF value as JSON (what Chromium's tree updates are made of).
pub fn to_json(value: &Value) -> Json {
    to_json_at(value, 0)
}

fn to_json_at(value: &Value, depth: usize) -> Json {
    if depth > MAX_DEPTH {
        return Json::Null;
    }
    match value.get_type() {
        ValueType::BOOL => Json::Bool(value.bool() != 0),
        ValueType::INT => Json::from(value.int()),
        ValueType::DOUBLE => Json::from(value.double()),
        ValueType::STRING => Json::String(CefString::from(&value.string()).to_string()),
        ValueType::DICTIONARY => {
            value.dictionary().map_or(Json::Null, |d| dictionary(&d, depth + 1))
        }
        ValueType::LIST => value.list().map_or(Json::Null, |l| list(&l, depth + 1)),
        _ => Json::Null,
    }
}

fn dictionary(dictionary: &DictionaryValue, depth: usize) -> Json {
    let mut keys = CefStringList::new();
    dictionary.keys(Some(&mut keys));
    let mut out = serde_json::Map::new();
    for key in keys {
        if let Some(value) = dictionary.value(Some(&key.as_str().into())) {
            out.insert(key, to_json_at(&value, depth));
        }
    }
    Json::Object(out)
}

fn list(list: &ListValue, depth: usize) -> Json {
    let items = (0..list.size()).filter_map(|i| list.value(i)).map(|v| to_json_at(&v, depth));
    Json::Array(items.collect())
}

#[cfg(test)]
#[path = "../tests/unit/access.rs"]
mod tests;
