//! What a window shows, as the system's assistive technologies read it (a screen reader, a
//! braille display, voice control): a tree of nodes, each with its role, its name, its value,
//! its state and where it is. The UI puts the nodes as it paints (each shared component its
//! own), the platform hands the tree to the system and brings back what the user asked of a
//! node (press it, focus it, set its value).
//!
//! A frame's tree is made only while the system asks for one (a screen reader running);
//! otherwise nothing is made. Between frames only the nodes that changed are told.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use kernel::Rect;

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The window's own root.
    Window,
    /// A group of nodes with a name of its own (a panel, a card, a toolbar).
    Group,
    /// A modal window over the others.
    Dialog,
    Button,
    /// A button that stays pressed or not (a toggle).
    ToggleButton,
    CheckBox,
    Switch,
    Radio,
    /// A field of one line of text.
    TextField,
    /// A field of several lines (the code editor).
    TextArea,
    SearchField,
    /// A field showing a choice, its list opened by a press.
    ComboBox,
    List,
    ListItem,
    Tree,
    TreeItem,
    Table,
    Row,
    Cell,
    TabList,
    Tab,
    Menu,
    MenuItem,
    ProgressBar,
    Slider,
    /// A number stepped up or down (a field with its arrows).
    SpinButton,
    /// A scrollbar: its value the share scrolled.
    ScrollBar,
    /// A separator between panels, dragged to resize them.
    Splitter,
    /// A table's column's header (its name, a click sorts by it).
    ColumnHeader,
    /// A text to read (a label, a heading, a message).
    Label,
    Heading,
    Link,
    Image,
    /// A message told once as it appears (a notification).
    Alert,
}

/// A node's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct State {
    /// Can't be used now (greyed out).
    pub disabled: bool,
    /// Chosen (a row, a tab, a segment).
    pub selected: bool,
    /// A checkbox's, a switch's, a toggle's: `None` for a node that has none.
    pub checked: Option<bool>,
    /// A checkbox standing for a group partly chosen (neither checked nor not).
    pub mixed: bool,
    /// Opened (a fold, a dropdown's list): `None` for a node that opens nothing.
    pub expanded: Option<bool>,
    /// It has the keyboard.
    pub focused: bool,
}

/// What the user can ask of a node through the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Actions {
    /// Pressed (a button, a row, a tab): as a click in its middle.
    pub press: bool,
    /// Given the keyboard.
    pub focus: bool,
    /// Its value set (a field's text).
    pub set_value: bool,
}

/// A node's identity: the same for the same thing from frame to frame (where it is in the
/// tree and what it's called), so the system follows it (the focus stays on it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

/// The window's root.
pub const ROOT: NodeId = NodeId(0);

/// A node as the UI put it.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub parent: NodeId,
    pub role: Role,
    pub name: Arc<str>,
    pub value: Option<Arc<str>>,
    pub state: State,
    pub actions: Actions,
    /// Where it is in the window (physical pixels).
    pub bounds: Rect,
    pub place: Place,
    /// A field's text as it shows: its lines and its caret.
    pub lines: Option<Box<TextLines>>,
    /// The keys that do what it does ("⌘S").
    pub shortcut: Option<Arc<str>>,
}

/// Where a node stands among its own: an item's place in its list, a list's length, a
/// tree item's depth ("3 of 10", "level 2"). All counted from zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Place {
    /// An item's (a list's row, a menu's, a table's): its index among all of them, the
    /// ones scrolled away counted.
    pub index: Option<usize>,
    /// A list's, a menu's, a table's, a tree's: how many items it has, shown or not.
    pub count: Option<usize>,
    /// A tree item's depth (its top level is 0).
    pub level: Option<usize>,
}

/// A field's text as the screen shows it: the lines of its value (split at `\n`), where
/// each one is, the selection in them. A code editor tells the lines it shows, never the
/// whole file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextLines {
    /// Each line's box (physical pixels), one per line of the value.
    pub lines: Vec<Rect>,
    /// Where the selection began and where it ends (the caret), bytes in the value: the
    /// same twice for a caret alone.
    pub anchor: usize,
    pub focus: usize,
    /// The caret's box: pressed there, the field takes the keyboard without moving it.
    pub caret: Option<Rect>,
}

/// What a component tells of itself (the node without its place in the tree).
#[derive(Debug, Clone, PartialEq)]
pub struct Described {
    pub role: Role,
    pub name: Arc<str>,
    pub value: Option<Arc<str>>,
    pub state: State,
    pub actions: Actions,
    pub bounds: Rect,
    /// What tells it apart from its siblings when its name doesn't (a row's index, a
    /// setting's key): its identity holds while its name changes.
    pub key: Option<u64>,
    pub place: Place,
    pub lines: Option<Box<TextLines>>,
    pub shortcut: Option<Arc<str>>,
}

impl Described {
    /// A node of `role` named `name` at `bounds`, with the actions its role usually has.
    pub fn new(role: Role, name: impl Into<Arc<str>>, bounds: Rect) -> Self {
        let actions = match role {
            Role::Button
            | Role::ToggleButton
            | Role::CheckBox
            | Role::Switch
            | Role::Radio
            | Role::ComboBox
            | Role::ListItem
            | Role::TreeItem
            | Role::Row
            | Role::Tab
            | Role::MenuItem
            | Role::Link => Actions { press: true, focus: true, set_value: false },
            Role::TextField | Role::TextArea | Role::SearchField | Role::SpinButton => {
                Actions { press: true, focus: true, set_value: true }
            }
            Role::ColumnHeader => Actions { press: true, focus: false, set_value: false },
            _ => Actions::default(),
        };
        Self {
            role,
            name: name.into(),
            value: None,
            state: State::default(),
            actions,
            bounds,
            key: None,
            place: Place::default(),
            lines: None,
            shortcut: None,
        }
    }

    pub fn value(mut self, value: impl Into<Arc<str>>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }

    /// An item's index among all of its list's (from zero).
    pub fn index(mut self, index: usize) -> Self {
        self.place.index = Some(index);
        self
    }

    /// A list's (a menu's, a table's, a tree's) number of items, shown or not.
    pub fn count(mut self, count: usize) -> Self {
        self.place.count = Some(count);
        self
    }

    /// A tree item's depth (from zero).
    pub fn level(mut self, level: usize) -> Self {
        self.place.level = Some(level);
        self
    }

    /// The keys that do what it does ("⌘S").
    pub fn shortcut(mut self, keys: impl Into<Arc<str>>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// A field's text as it shows (its lines, its caret).
    pub fn lines(mut self, lines: TextLines) -> Self {
        self.lines = Some(Box::new(lines));
        self
    }

    pub fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled = disabled;
        if disabled {
            self.actions = Actions::default();
        }
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.state.checked = Some(checked);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.state.selected = selected;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.state.focused = focused;
        self
    }

    /// Its text tells `text` already (its name or its value holds it): a label drawn in it
    /// saying so isn't read a second time.
    fn tells(&self, text: &str) -> bool {
        // A text cut short to fit ("index.ph…", "…/src/main.rs") says what its rest says.
        let text = text.trim_matches('…');
        let holds = |said: Option<&str>| said.is_some_and(|s| s.contains(text));
        !text.is_empty()
            && (self.name.contains(text)
                || holds(self.value.as_deref())
                || holds(self.shortcut.as_deref()))
    }
}

/// Two pieces of text this far apart at most (times their height) are one word's pieces;
/// farther, words, a space between them.
const PIECE_GAP: f32 = 0.15;

/// How far apart two pieces of one line may stand: this many times their height.
const PIECE_REACH: f32 = 1.5;

/// `b` follows `a` on its line: their middles at one height, `b` starting after `a` ends,
/// within [`PIECE_REACH`] of its height.
fn same_line(a: &Rect, b: &Rect) -> bool {
    let h = a.size.height.max(1.0);
    (a.center().y - b.center().y).abs() <= h * 0.25
        && b.min_x() >= a.max_x() - h * 0.1
        && b.min_x() - a.max_x() <= h * PIECE_REACH
}

/// How far from a control the text naming it may be: this many times its height.
const LABEL_REACH: f32 = 12.0;

/// A group open while the UI paints (see [`Frame::open`]).
#[derive(Debug, Clone)]
struct Open {
    id: NodeId,
    role: Role,
    /// A control or a message with a name: its texts that say it are its own.
    owner: Option<Described>,
}

/// A frame's tree, put together as the UI paints: nodes under the group open at the time.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    nodes: Vec<Node>,
    /// The groups open now (the last one is where a node goes).
    open: Vec<Open>,
    /// How many nodes of a kind a parent has had so far: two "OK" buttons in one dialog are
    /// told apart.
    seen: HashMap<(NodeId, u64), u32>,
    /// The last control put with a name, where it is and under which parent: a text drawn
    /// inside it that says its name (a menu row's label) is its own.
    last_control: Option<(NodeId, Described)>,
}

impl Frame {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empty again for the next frame (its memory kept).
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.open.clear();
        self.seen.clear();
        self.last_control = None;
    }

    /// `node` put under the group open now; its identity. A text that a control around it
    /// (the group open, or the last control it's drawn inside) says already isn't put again.
    pub fn add(&mut self, node: Described) -> NodeId {
        let parent = self.open.last().map_or(ROOT, |g| g.id);
        if node.role == Role::Label && self.told(parent, &node) {
            return parent;
        }
        if node.actions.press && !node.name.is_empty() {
            self.last_control = Some((parent, node.clone()));
        }
        let id = self.identity(parent, &node);
        self.nodes.push(Node {
            id,
            parent,
            role: node.role,
            name: node.name,
            value: node.value,
            state: node.state,
            actions: node.actions,
            bounds: node.bounds,
            place: node.place,
            lines: node.lines,
            shortcut: node.shortcut,
        });
        id
    }

    /// `label` is said already by the control it's drawn in.
    fn told(&self, parent: NodeId, label: &Described) -> bool {
        if let Some(owner) = self.open.last().and_then(|g| g.owner.as_ref()) {
            // A dialog holds texts of all kinds: only its title is its own.
            if owner.role == Role::Dialog {
                return owner.name == label.name;
            }
            return owner.tells(&label.name);
        }
        let middle = label.bounds.center();
        self.last_control.as_ref().is_some_and(|(at, control)| {
            *at == parent && control.bounds.contains(middle) && control.tells(&label.name)
        })
    }

    /// `group` put and opened: the nodes after it go under it until [`Self::close`].
    pub fn open(&mut self, group: Described) -> NodeId {
        // A control or a message with a name owns the texts that say it, pressable or not (a
        // greyed-out button's label is its name too); a container (a group, a list) doesn't.
        let container = matches!(
            group.role,
            Role::Group
                | Role::List
                | Role::Table
                | Role::Tree
                | Role::TabList
                | Role::Menu
                | Role::Row
        );
        let owner = (!container && !group.name.is_empty()).then(|| group.clone());
        let role = group.role;
        let id = self.add(group);
        self.open.push(Open { id, role, owner });
        self.last_control = None;
        id
    }

    /// The role of the group open now (`None` at the window's top).
    pub fn open_role(&self) -> Option<Role> {
        self.open.last().map(|g| g.role)
    }

    /// The group opened last, closed.
    pub fn close(&mut self) {
        self.open.pop();
        self.last_control = None;
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The same nodes as `other` (nothing to tell the system again).
    pub fn same_as(&self, other: &Frame) -> bool {
        self.nodes == other.nodes
    }

    /// Each parent's children (their places in [`Self::nodes`]), in order.
    pub fn children(&self) -> HashMap<NodeId, Vec<usize>> {
        let mut children: HashMap<NodeId, Vec<usize>> = HashMap::with_capacity(self.nodes.len());
        for (i, n) in self.nodes.iter().enumerate() {
            children.entry(n.parent).or_default().push(i);
        }
        children
    }

    /// The node with the keyboard, if one said so.
    pub fn focused(&self) -> Option<NodeId> {
        self.nodes.iter().rev().find(|n| n.state.focused).map(|n| n.id)
    }

    /// A node's identity: its parent's, its role, its key (else its name), and how many such
    /// it follows under that parent (never where it is: a list scrolled keeps its rows').
    fn identity(&mut self, parent: NodeId, node: &Described) -> NodeId {
        let mut kind = std::collections::hash_map::DefaultHasher::new();
        node.role.hash(&mut kind);
        match node.key {
            Some(key) => key.hash(&mut kind),
            None => node.name.hash(&mut kind),
        }
        let kind = kind.finish();
        let nth = self.seen.entry((parent, kind)).or_insert(0);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (parent, kind, *nth).hash(&mut h);
        *nth += 1;
        // Never the root's.
        NodeId(h.finish().max(1))
    }

    /// The texts drawn in pieces on one line (a paragraph wrapped word by word, a name with
    /// its match lit) joined into one, as it reads: a space put back where the pieces stand
    /// apart, none where they touch. Only pieces put one after the other, under the same
    /// parent, on the same line and close by are joined.
    pub fn join_pieces(&mut self) {
        let mut kept: Vec<Node> = Vec::with_capacity(self.nodes.len());
        for n in self.nodes.drain(..) {
            if let Some(last) = kept.last_mut()
                && n.role == Role::Label
                && last.role == Role::Label
                && last.parent == n.parent
                && same_line(&last.bounds, &n.bounds)
            {
                let gap = n.bounds.min_x() - last.bounds.max_x();
                let touching = gap <= last.bounds.size.height * PIECE_GAP
                    || last.name.ends_with(char::is_whitespace)
                    || n.name.starts_with(char::is_whitespace);
                let mut name = String::with_capacity(last.name.len() + n.name.len() + 1);
                name.push_str(&last.name);
                if !touching {
                    name.push(' ');
                }
                name.push_str(&n.name);
                last.name = Arc::from(name);
                last.bounds = last.bounds.union(&n.bounds);
                continue;
            }
            kept.push(n);
        }
        self.nodes = kept;
    }

    /// The controls put without a name (a switch, a checkbox, a dropdown: their label is
    /// drawn beside them by whoever uses them) named by the text on their line under the
    /// same parent: the nearest within [`LABEL_REACH`] of their height, before them first.
    /// Their identities follow the name found (not their order among the unnamed), and
    /// what's under them follows.
    pub fn name_from_labels(&mut self) {
        // Each parent's texts, by the height of their middle.
        let mut labels: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if n.role == Role::Label {
                labels.entry(n.parent).or_default().push(i);
            }
        }
        for line in labels.values_mut() {
            line.sort_by(|&a, &b| {
                self.nodes[a].bounds.center().y.total_cmp(&self.nodes[b].bounds.center().y)
            });
        }
        let mut renamed: HashMap<NodeId, NodeId> = HashMap::new();
        for i in 0..self.nodes.len() {
            let n = &self.nodes[i];
            if !n.name.is_empty() || !labelled_beside(n.role) {
                continue;
            }
            let Some(line) = labels.get(&n.parent) else { continue };
            let Some(name) = nearest_label(&self.nodes, line, n.bounds) else { continue };
            self.rename(i, name, &mut renamed);
        }
        self.follow(&renamed);
    }

    /// The controls still without a name (a button showing only an icon) named by what the
    /// pointer would be told over them (`tip`: their tooltip and its keys), their identities
    /// following, as [`Self::name_from_labels`].
    pub fn name_from_tips(
        &mut self,
        mut tip: impl FnMut(Rect) -> Option<(String, Option<String>)>,
    ) {
        let mut renamed: HashMap<NodeId, NodeId> = HashMap::new();
        for i in 0..self.nodes.len() {
            let n = &self.nodes[i];
            if !n.name.is_empty() || !n.actions.press {
                continue;
            }
            let Some((name, keys)) = tip(n.bounds) else { continue };
            if name.is_empty() {
                continue;
            }
            if self.nodes[i].shortcut.is_none() {
                self.nodes[i].shortcut = keys.map(Arc::from);
            }
            self.rename(i, Arc::from(name), &mut renamed);
        }
        self.follow(&renamed);
    }

    /// Node `i` named `name`, its identity made anew from it (the old one noted in `renamed`).
    fn rename(&mut self, i: usize, name: Arc<str>, renamed: &mut HashMap<NodeId, NodeId>) {
        let n = &self.nodes[i];
        let (old, parent) = (n.id, n.parent);
        let described = Described::new(n.role, name, n.bounds);
        let id = self.identity(parent, &described);
        self.nodes[i].name = described.name;
        self.nodes[i].id = id;
        renamed.insert(old, id);
    }

    /// The children of the nodes `renamed` under their new identities.
    fn follow(&mut self, renamed: &HashMap<NodeId, NodeId>) {
        if renamed.is_empty() {
            return;
        }
        for n in &mut self.nodes {
            if let Some(&id) = renamed.get(&n.parent) {
                n.parent = id;
            }
        }
    }
}

/// The text among `line` (a parent's texts, by height) naming a control at `control`: on
/// its line, the nearest before it, else after it, within [`LABEL_REACH`] of its height.
fn nearest_label(nodes: &[Node], line: &[usize], control: Rect) -> Option<Arc<str>> {
    let (top, bottom) = (control.min_y(), control.max_y());
    let reach = control.size.height.max(1.0) * LABEL_REACH;
    let from = line.partition_point(|&i| nodes[i].bounds.center().y < top);
    let distance = |label: &Rect| {
        if label.max_x() <= control.min_x() {
            control.min_x() - label.max_x()
        } else {
            // After it: farther than any before it at the same distance.
            (label.min_x() - control.max_x()).max(0.0) + 0.5
        }
    };
    line[from..]
        .iter()
        .map(|&i| &nodes[i])
        .take_while(|n| n.bounds.center().y <= bottom)
        .map(|n| (distance(&n.bounds), n))
        .filter(|(d, _)| *d <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, n)| Arc::clone(&n.name))
}

/// A control whose label is drawn beside it by whoever uses it (a row's, a list's items
/// carry their own texts inside them; a button without one is told by its tooltip, never by
/// a title near it).
fn labelled_beside(role: Role) -> bool {
    matches!(
        role,
        Role::CheckBox
            | Role::Switch
            | Role::Radio
            | Role::ComboBox
            | Role::TextField
            | Role::SearchField
            | Role::Slider
            | Role::TreeItem
    )
}

/// What the user asked of a node through the system.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Press(NodeId),
    Focus(NodeId),
    SetValue(NodeId, String),
    /// A selection in a field's text (a caret: both ends the same), each end a line of it
    /// (its run, [`run_id`]) and a character in that line.
    Select {
        field: NodeId,
        anchor: TextPlace,
        focus: TextPlace,
    },
}

/// A place in a field's text as the system tells it: a line's run ([`run_id`]) and the
/// character in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPlace {
    pub run: u64,
    pub character: usize,
}

/// The identity of line `line` of field `field`'s text, as the system reads it (a run of
/// text): never one of the app's nodes (theirs are hashes too, the root's 0; a run's has its
/// top bit set).
pub fn run_id(field: NodeId, line: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (field, line).hash(&mut h);
    h.finish() | 1 << 63
}

/// The byte in `text` (its lines as the system reads them: each with its break) of `place`,
/// line `line` of it (`None`: no such line).
pub fn byte_of(text: &str, line: usize, character: usize) -> Option<usize> {
    let mut start = 0;
    for (i, part) in text.split_inclusive('\n').enumerate() {
        if i == line {
            let inside = part.char_indices().nth(character).map_or(part.len(), |(at, _)| at);
            return Some(start + inside);
        }
        start += part.len();
    }
    // An empty text, or the empty line after a last break.
    (line == text.split_inclusive('\n').count() || text.is_empty() && line == 0).then_some(start)
}

#[cfg(test)]
#[path = "../tests/unit/access.rs"]
mod tests;
