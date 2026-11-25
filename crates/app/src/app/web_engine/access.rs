//! What a page's engine says as the window has it: its accessibility tree's nodes and the
//! pointer it asks for.

use web_engine::protocol::Cursor;

/// A node of a page's tree as the window's tree has it: Chromium's role as ours (what has
/// none is told as a group, or a text when it's only that), its state.
pub(super) fn access_node(node: web_engine::protocol::AccessNode) -> platform_api::PageAccessNode {
    use platform_api::access::{Role, State};
    let role = match node.role.as_str() {
        "button" | "popUpButton" => Role::Button,
        "toggleButton" => Role::ToggleButton,
        "checkBox" | "menuItemCheckBox" => Role::CheckBox,
        "switch" => Role::Switch,
        "radioButton" | "menuItemRadio" => Role::Radio,
        "textField" | "textFieldWithComboBox" => Role::TextField,
        "searchBox" => Role::SearchField,
        "comboBoxSelect" | "comboBoxMenuButton" | "comboBoxGrouping" => Role::ComboBox,
        "list" | "listBox" | "descriptionList" => Role::List,
        "listItem" | "listBoxOption" | "descriptionListTerm" => Role::ListItem,
        "tree" => Role::Tree,
        "treeItem" => Role::TreeItem,
        "table" | "grid" | "treeGrid" => Role::Table,
        "row" => Role::Row,
        "cell" | "gridCell" => Role::Cell,
        "columnHeader" | "rowHeader" => Role::ColumnHeader,
        "tabList" => Role::TabList,
        "tab" => Role::Tab,
        "menu" | "menuBar" | "menuListPopup" => Role::Menu,
        "menuItem" | "menuListOption" => Role::MenuItem,
        "progressIndicator" | "meter" => Role::ProgressBar,
        "slider" => Role::Slider,
        "spinButton" => Role::SpinButton,
        "scrollBar" => Role::ScrollBar,
        "splitter" => Role::Splitter,
        "heading" => Role::Heading,
        "link" => Role::Link,
        "image" | "svgRoot" | "canvas" => Role::Image,
        "alert" | "alertDialog" => Role::Alert,
        "dialog" => Role::Dialog,
        "staticText" | "labelText" | "paragraph" => Role::Label,
        _ => Role::Group,
    };
    // A field holding several lines is a text area.
    let lines = node.value.as_deref().is_some_and(|v| v.contains('\n'));
    let role = if role == Role::TextField && lines { Role::TextArea } else { role };
    let state = State {
        disabled: node.disabled,
        selected: node.selected,
        checked: node.checked,
        expanded: node.expanded,
        focused: node.focused,
        ..State::default()
    };
    let [x, y, width, height] = node.rect;
    platform_api::PageAccessNode {
        id: node.id,
        parent: node.parent,
        role,
        name: node.name.into(),
        value: node.value.map(Into::into),
        state,
        level: node.level.map(usize::from),
        bounds: kernel::Rect::new(x, y, width, height),
    }
}

/// The window's pointer for the one a page asks (one the window has none like: its own).
pub(super) fn cursor_of(cursor: Cursor) -> platform_api::CursorIcon {
    use platform_api::CursorIcon as Icon;
    match cursor {
        Cursor::Pointer => Icon::Pointer,
        Cursor::Text => Icon::Text,
        Cursor::Crosshair => Icon::Crosshair,
        Cursor::Move => Icon::Move,
        Cursor::Grab => Icon::Grab,
        Cursor::Grabbing => Icon::Grabbing,
        Cursor::ResizeHorizontal => Icon::ResizeHorizontal,
        Cursor::ResizeVertical => Icon::ResizeVertical,
        Cursor::ResizeDiagonal => Icon::ResizeDiagonal,
        Cursor::ResizeAntiDiagonal => Icon::ResizeAntiDiagonal,
        Cursor::NotAllowed => Icon::NotAllowed,
        Cursor::Wait => Icon::Wait,
        Cursor::Progress => Icon::Progress,
        Cursor::Help => Icon::Help,
        Cursor::Arrow | Cursor::None => Icon::Default,
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_engine/access.rs"]
mod tests;
