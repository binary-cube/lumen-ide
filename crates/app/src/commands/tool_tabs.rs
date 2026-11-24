//! The menu of a bottom window's tab: its actions (rename, pin, close…), as commands with
//! the tab in their id.

use super::*;

/// An action of a bottom window tab's menu (any window's: the terminal's shells, Git's
/// views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTabAction {
    Rename,
    TogglePin,
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseAll,
    SplitRight,
    SplitDown,
    SplitMoveRight,
    SplitMoveDown,
}

impl ToolTabAction {
    const ALL: [(Self, &'static str); 11] = [
        (Self::Rename, "rename"),
        (Self::TogglePin, "pin"),
        (Self::Close, "close"),
        (Self::CloseOthers, "close-others"),
        (Self::CloseLeft, "close-left"),
        (Self::CloseRight, "close-right"),
        (Self::CloseAll, "close-all"),
        (Self::SplitRight, "split-right"),
        (Self::SplitDown, "split-down"),
        (Self::SplitMoveRight, "split-move-right"),
        (Self::SplitMoveDown, "split-move-down"),
    ];

    fn name(self) -> &'static str {
        Self::ALL.iter().find(|(a, _)| *a == self).map_or("", |(_, n)| n)
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::Rename => "file-pen",
            Self::TogglePin => "pin",
            Self::Close | Self::CloseLeft | Self::CloseRight | Self::CloseAll => "x",
            Self::CloseOthers => "x-square",
            Self::SplitRight => "view-split",
            Self::SplitMoveRight => "view-split-move",
            Self::SplitDown => "view-split-down",
            Self::SplitMoveDown => "view-split-move-down",
        }
    }
}

/// A bottom window's tab, as its menu sees it: its place among the `count` of its group,
/// pinned, closable (a fixed one isn't).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolTab {
    pub index: usize,
    pub count: usize,
    pub pinned: bool,
    pub closable: bool,
}

/// The context menu of a bottom window's tab: rename, pin, the closes (on its group; a
/// fixed tab has no Close of its own), the splits — another view in a new group when the
/// window makes them (`makes_views`: the terminal's shells), or this one moved there when
/// its group has others.
pub fn tool_tab_menu(tab: ToolTab, makes_views: bool) -> Vec<MenuItem> {
    use ToolTabAction as A;
    let ToolTab { index, count, pinned, closable } = tab;
    let item = |action: A, label: &str| {
        MenuItem::command(&format!("{TOOL_TAB_PREFIX}{}:{index}", action.name()), label, None)
    };
    let pin = if pinned {
        localized("command.unpin_tab", "Unpin Tab")
    } else {
        localized("command.tab_ctx_pin", "Pin Tab")
    };
    let mut items = vec![
        item(A::Rename, &localized("command.rename", "Rename…")),
        item(A::TogglePin, &pin),
        MenuItem::Separator,
    ];
    if closable {
        items.push(item(A::Close, &label(TAB_CLOSE)));
    }
    if count > 1 {
        items.push(item(
            A::CloseOthers,
            &localized("command.close_other_views", "Close Other Tabs"),
        ));
        if index > 0 {
            items.push(item(A::CloseLeft, &label(TAB_CLOSE_LEFT)));
        }
        if index + 1 < count {
            items.push(item(A::CloseRight, &label(TAB_CLOSE_RIGHT)));
        }
        items.push(item(A::CloseAll, &label(TAB_CLOSE_ALL)));
    }
    let mut splits = Vec::new();
    for (split, moved) in [(A::SplitRight, A::SplitMoveRight), (A::SplitDown, A::SplitMoveDown)] {
        let (id, moved_id) = match split {
            A::SplitRight => (SPLIT_RIGHT, SPLIT_MOVE_RIGHT),
            _ => (SPLIT_DOWN, SPLIT_MOVE_DOWN),
        };
        if makes_views {
            splits.push(item(split, &label(id)));
        }
        if count > 1 {
            splits.push(item(moved, &label(moved_id)));
        }
    }
    if !splits.is_empty() {
        items.push(MenuItem::Separator);
        items.extend(splits);
    }
    items
}

/// `tool-tab:<action>:<index>` → (action, index).
pub fn parse_tool_tab(id: &str) -> Option<(ToolTabAction, usize)> {
    let (name, index) = id.strip_prefix(TOOL_TAB_PREFIX)?.split_once(':')?;
    let action = ToolTabAction::ALL.iter().find(|(_, n)| *n == name)?.0;
    Some((action, index.parse().ok()?))
}
