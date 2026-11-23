//! The window's accessibility tree handed to the system (`platform_api::access`) through
//! AccessKit: UI Automation on Windows, NSAccessibility on macOS, AT-SPI on Linux. The
//! adapter is made with the window (before it shows); the system asking for the tree turns
//! it on (the app then makes one each frame that changed it), a screen reader leaving turns
//! it off. What the user asks of a node comes back to the app as an event.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use accesskit::{
    Action, Node as KitNode, NodeId as KitId, Rect as KitRect, Role as KitRole, Toggled,
};
use accesskit::{TextPosition, TextSelection, TreeId, TreeInfo, TreeUpdate};
use platform_api::access::{Frame, NodeId, Place, ROOT, Request, Role, TextLines};

/// A window's adapter and whether the system wants its tree.
pub(crate) struct Access {
    pub adapter: accesskit_winit::Adapter,
    pub asks: Asks,
}

/// The system's asks for a window's tree: which one is on now (0: none, it doesn't want
/// it), read by the app before each frame; counted on, never back — an ask after a screen
/// reader left is a new one, told the whole tree, never a change from the last one's.
#[derive(Debug, Default)]
pub(crate) struct Asks {
    pub asked: Arc<AtomicU64>,
    count: u64,
}

impl Asks {
    /// The system asks for the tree (again: the whole of it is told).
    pub fn asked(&mut self) {
        self.count = self.count.wrapping_add(1).max(1);
        self.asked.store(self.count, Ordering::Relaxed);
    }

    /// Whether an update made for ask `ask` is for the one on now (one made before the
    /// system asked anew is left: it's a change from a tree this one never had).
    pub fn current(&self, ask: u64) -> bool {
        ask != 0 && ask == self.asked.load(Ordering::Relaxed)
    }

    /// The system no longer wants it (its screen reader left).
    pub fn left(&self) {
        self.asked.store(0, Ordering::Relaxed);
    }
}

/// The app's tree as AccessKit takes it, the window as its root, the focus on the node said
/// focused (the window when none is): the whole of it (`previous` `None`: the system asked
/// anew), else only the nodes new or changed since `previous` (a node the same, with the
/// same children, isn't made again nor sent; one gone leaves with its parent's new
/// children). A busy screen's thousands of nodes cost a few hundred µs to make whole; most
/// frames change a handful.
pub(crate) fn tree_update(frame: &Frame, title: &str, previous: Option<&Frame>) -> TreeUpdate {
    let nodes = frame.nodes();
    let ids = |places: &[usize]| places.iter().map(|&i| KitId(nodes[i].id.0)).collect::<Vec<_>>();
    // What was told last: where each node was, and a print of each one's children (no list
    // made: an id folded in order); this frame's children's prints the same way.
    let told = previous.map(|before| (places(before), kid_prints(before), before.nodes()));
    let prints = told.as_ref().map(|_| kid_prints(frame));
    let unchanged = |n: &platform_api::access::Node| {
        let (Some((at, kids, was)), Some(prints)) = (&told, &prints) else { return false };
        at.get(&n.id).is_some_and(|&i| was[i] == *n) && kids.get(&n.id) == prints.get(&n.id)
    };
    // The nodes to send, and the children of those only (and the window's).
    let sent: Vec<bool> = nodes.iter().map(|n| !unchanged(n)).collect();
    let mut children = if told.is_none() {
        frame.children()
    } else {
        let wanted: std::collections::HashSet<NodeId> =
            nodes.iter().zip(&sent).filter(|(_, s)| **s).map(|(n, _)| n.id).chain([ROOT]).collect();
        let mut some: std::collections::HashMap<NodeId, Vec<usize>> =
            std::collections::HashMap::with_capacity(wanted.len());
        for (i, n) in nodes.iter().enumerate() {
            if wanted.contains(&n.parent) {
                some.entry(n.parent).or_default().push(i);
            }
        }
        some
    };
    let mut update = Vec::with_capacity(nodes.len() + 1);
    let mut root = KitNode::new(KitRole::Window);
    root.set_label(title);
    root.set_children(children.remove(&ROOT).map(|c| ids(&c)).unwrap_or_default());
    update.push((KitId(ROOT.0), root));
    for (n, _) in nodes.iter().zip(&sent).filter(|(_, s)| **s) {
        let mut node = KitNode::new(role(n.role));
        if !n.name.is_empty() {
            node.set_label(&*n.name);
        }
        if let Some(value) = &n.value {
            node.set_value(&**value);
        }
        node.set_bounds(kit_rect(n.bounds));
        if let Some(keys) = &n.shortcut {
            node.set_keyboard_shortcut(&**keys);
        }
        place(&mut node, n.role, n.place);
        if n.state.disabled {
            node.set_disabled();
        }
        // A dialog over the window: what's behind it isn't read while it's open.
        if n.role == Role::Dialog {
            node.set_modal();
        }
        if n.state.selected {
            node.set_selected(true);
        }
        if n.state.mixed {
            node.set_toggled(Toggled::Mixed);
        } else if let Some(checked) = n.state.checked {
            node.set_toggled(if checked { Toggled::True } else { Toggled::False });
        }
        if let Some(expanded) = n.state.expanded {
            node.set_expanded(expanded);
        }
        if n.actions.press {
            node.add_action(Action::Click);
        }
        if n.actions.focus {
            node.add_action(Action::Focus);
        }
        if n.actions.set_value {
            node.add_action(Action::SetValue);
        }
        // A field telling its text by lines: its caret and selection may be set from there.
        if n.lines.is_some() {
            node.add_action(Action::SetTextSelection);
        }
        let mut kids = children.remove(&n.id).map(|c| ids(&c)).unwrap_or_default();
        if let (Some(lines), Some(value)) = (&n.lines, &n.value) {
            text_runs(n.id, lines, value, &mut node, &mut kids, &mut update);
        }
        if !kids.is_empty() {
            node.set_children(kids);
        }
        update.push((KitId(n.id.0), node));
    }
    TreeUpdate {
        nodes: update,
        tree: previous.is_none().then(|| TreeInfo::new(KitId(ROOT.0))),
        tree_id: TreeId::ROOT,
        focus: KitId(frame.focused().unwrap_or(ROOT).0),
    }
}

/// Where each node of `frame` is (its place in the frame's list), by its identity.
fn places(frame: &Frame) -> std::collections::HashMap<NodeId, usize> {
    frame.nodes().iter().enumerate().map(|(i, n)| (n.id, i)).collect()
}

/// A print of each node's children, in their order (a parent without one isn't in it): two
/// frames' match when a node has the same children.
fn kid_prints(frame: &Frame) -> std::collections::HashMap<NodeId, u64> {
    let mut prints = std::collections::HashMap::with_capacity(frame.nodes().len());
    for n in frame.nodes() {
        let print: &mut u64 = prints.entry(n.parent).or_insert(0xcbf2_9ce4_8422_2325);
        *print = (*print ^ n.id.0).wrapping_mul(0x0100_0000_01b3);
    }
    prints
}

fn kit_rect(b: kernel::Rect) -> KitRect {
    KitRect {
        x0: f64::from(b.min_x()),
        y0: f64::from(b.min_y()),
        x1: f64::from(b.max_x()),
        y1: f64::from(b.max_y()),
    }
}

/// A node's place as the system reads it: a table's rows by their index and count, any
/// other item by its place in its set, a tree's by its level.
fn place(node: &mut KitNode, role: Role, place: Place) {
    match role {
        Role::Table => {
            if let Some(count) = place.count {
                node.set_row_count(count);
            }
        }
        Role::Row => {
            if let Some(index) = place.index {
                node.set_row_index(index);
            }
        }
        // A cell and its column's header by the column's place: read together.
        Role::Cell | Role::ColumnHeader => {
            if let Some(index) = place.index {
                node.set_column_index(index);
            }
        }
        _ => {
            if let Some(count) = place.count {
                node.set_size_of_set(count);
            }
            if let Some(index) = place.index {
                node.set_position_in_set(index);
            }
        }
    }
    if let Some(level) = place.level {
        node.set_level(level);
    }
}

/// A run's identity: its field's and its line's (`platform_api::access::run_id`).
fn run_id(field: NodeId, line: usize) -> KitId {
    KitId(platform_api::access::run_id(field, line))
}

/// A field's text as the system reads a text: a run a line (its characters' lengths, its
/// box, its line break counted as one character), and the selection in them.
fn text_runs(
    field_id: NodeId,
    lines: &TextLines,
    value: &str,
    field: &mut KitNode,
    kids: &mut Vec<KitId>,
    update: &mut Vec<(KitId, KitNode)>,
) {
    let mut start = 0;
    let mut anchor = None;
    let mut focus = None;
    let parts: Vec<&str> = value.split_inclusive('\n').collect();
    let count = parts.len().max(1);
    for (i, part) in parts.iter().copied().chain((parts.is_empty()).then_some("")).enumerate() {
        let id = run_id(field_id, i);
        let mut run = KitNode::new(KitRole::TextRun);
        run.set_value(part);
        let mut lengths: Vec<u8> = Vec::with_capacity(part.len());
        let mut offsets: Vec<usize> = Vec::with_capacity(part.len());
        for (at, c) in part.char_indices() {
            offsets.push(at);
            lengths.push(u8::try_from(c.len_utf8()).unwrap_or(1));
        }
        // A byte in this line (its end only on the last: the next line's start is its).
        let end = start + part.len();
        let last = i + 1 == count;
        let index_of = |byte: usize| -> Option<usize> {
            let inside = byte >= start && (byte < end || (last && byte == end));
            inside.then(|| offsets.partition_point(|&o| start + o < byte))
        };
        if anchor.is_none() {
            anchor = index_of(lines.anchor).map(|c| TextPosition { node: id, character_index: c });
        }
        if focus.is_none() {
            focus = index_of(lines.focus).map(|c| TextPosition { node: id, character_index: c });
        }
        run.set_character_lengths(lengths);
        if let Some(&b) = lines.lines.get(i).or(lines.lines.last()) {
            run.set_bounds(kit_rect(b));
        }
        kids.push(id);
        update.push((id, run));
        start = end;
    }
    if let (Some(anchor), Some(focus)) = (anchor, focus) {
        field.set_text_selection(TextSelection { anchor, focus });
    }
}

/// What the system asked of a node, as the app takes it (`None`: an action it doesn't do).
pub(crate) fn request(asked: &accesskit::ActionRequest) -> Option<Request> {
    let id = NodeId(asked.target_node.0);
    match asked.action {
        Action::Click => Some(Request::Press(id)),
        Action::Focus => Some(Request::Focus(id)),
        Action::SetValue => match &asked.data {
            Some(accesskit::ActionData::Value(value)) => {
                Some(Request::SetValue(id, value.to_string()))
            }
            _ => None,
        },
        Action::SetTextSelection => match &asked.data {
            Some(accesskit::ActionData::SetTextSelection(selection)) => {
                let place = |p: &TextPosition| platform_api::access::TextPlace {
                    run: p.node.0,
                    character: p.character_index,
                };
                Some(Request::Select {
                    field: id,
                    anchor: place(&selection.anchor),
                    focus: place(&selection.focus),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

fn role(role: Role) -> KitRole {
    match role {
        Role::Window => KitRole::Window,
        Role::Group => KitRole::Group,
        Role::Dialog => KitRole::Dialog,
        Role::Button | Role::ToggleButton => KitRole::Button,
        Role::CheckBox => KitRole::CheckBox,
        Role::Switch => KitRole::Switch,
        Role::Radio => KitRole::RadioButton,
        Role::TextField => KitRole::TextInput,
        Role::TextArea => KitRole::MultilineTextInput,
        Role::SearchField => KitRole::SearchInput,
        Role::ComboBox => KitRole::ComboBox,
        Role::List => KitRole::List,
        Role::ListItem => KitRole::ListItem,
        Role::Tree => KitRole::Tree,
        Role::TreeItem => KitRole::TreeItem,
        Role::Table => KitRole::Table,
        Role::Row => KitRole::Row,
        Role::Cell => KitRole::Cell,
        Role::TabList => KitRole::TabList,
        Role::Tab => KitRole::Tab,
        Role::Menu => KitRole::Menu,
        Role::MenuItem => KitRole::MenuItem,
        Role::ProgressBar => KitRole::ProgressIndicator,
        Role::Slider => KitRole::Slider,
        Role::SpinButton => KitRole::SpinButton,
        Role::ScrollBar => KitRole::ScrollBar,
        Role::Splitter => KitRole::Splitter,
        Role::ColumnHeader => KitRole::ColumnHeader,
        Role::Label => KitRole::Label,
        Role::Heading => KitRole::Heading,
        Role::Link => KitRole::Link,
        Role::Image => KitRole::Image,
        Role::Alert => KitRole::Alert,
    }
}

#[cfg(test)]
#[path = "../tests/unit/access_bridge.rs"]
mod tests;
