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
use accesskit::{TreeId, TreeInfo, TreeUpdate};
use platform_api::access::{Frame, NodeId, ROOT, Request, Role};

/// A window's adapter and whether the system wants its tree.
pub(crate) struct Access {
    pub adapter: accesskit_winit::Adapter,
    /// How many times the system asked for the tree (0: it doesn't want it now); read by
    /// the app before each frame.
    pub asked: Arc<AtomicU64>,
}

impl Access {
    /// The system asks for the tree (again: the whole of it is told).
    pub fn asked(&self) {
        let next = self.asked.load(Ordering::Relaxed).wrapping_add(1).max(1);
        self.asked.store(next, Ordering::Relaxed);
    }

    /// The system no longer wants it (its screen reader left).
    pub fn left(&self) {
        self.asked.store(0, Ordering::Relaxed);
    }
}

/// The app's tree as AccessKit takes it: the whole tree (AccessKit compares it with the one
/// it holds and tells the system only what changed), the window as its root, the focus on
/// the node said focused (the window when none is).
pub(crate) fn tree_update(frame: &Frame, title: &str) -> TreeUpdate {
    let nodes = frame.nodes();
    let ids = |places: &[usize]| places.iter().map(|&i| KitId(nodes[i].id.0)).collect::<Vec<_>>();
    let mut children = frame.children();
    let mut update = Vec::with_capacity(nodes.len() + 1);
    let mut root = KitNode::new(KitRole::Window);
    root.set_label(title);
    root.set_children(children.remove(&ROOT).map(|c| ids(&c)).unwrap_or_default());
    update.push((KitId(ROOT.0), root));
    for n in nodes {
        let mut node = KitNode::new(role(n.role));
        if !n.name.is_empty() {
            node.set_label(&*n.name);
        }
        if let Some(value) = &n.value {
            node.set_value(&**value);
        }
        let b = n.bounds;
        node.set_bounds(KitRect {
            x0: f64::from(b.min_x()),
            y0: f64::from(b.min_y()),
            x1: f64::from(b.max_x()),
            y1: f64::from(b.max_y()),
        });
        if n.state.disabled {
            node.set_disabled();
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
        if let Some(kids) = children.remove(&n.id) {
            node.set_children(ids(&kids));
        }
        update.push((KitId(n.id.0), node));
    }
    TreeUpdate {
        nodes: update,
        tree: Some(TreeInfo::new(KitId(ROOT.0))),
        tree_id: TreeId::ROOT,
        focus: KitId(frame.focused().unwrap_or(ROOT).0),
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
