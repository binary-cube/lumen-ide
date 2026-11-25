//! What a page shows beside its frames, told to its app: its cursor, where its context menu
//! was asked for (Chromium's own can't show without a window), its accessibility tree.

use cef::rc::Rc as _;
use cef::*;
use web_engine_protocol::{Cursor, Event};

use super::Shared;
use crate::text_of;

wrap_display_handler! {
    pub struct DisplayBuilder {
        page: Shared,
    }

    impl DisplayHandler {
        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: *mut u8,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> ::std::os::raw::c_int {
            let page = &self.page.0;
            page.send(Event::Cursor { page: page.key.page, cursor: cursor_of(type_) });
            1
        }
    }
}

/// The protocol's cursor for CEF's.
fn cursor_of(cursor: CursorType) -> Cursor {
    match cursor {
        CursorType::HAND => Cursor::Pointer,
        CursorType::IBEAM => Cursor::Text,
        CursorType::CROSS => Cursor::Crosshair,
        CursorType::MOVE => Cursor::Move,
        CursorType::GRAB => Cursor::Grab,
        CursorType::GRABBING => Cursor::Grabbing,
        CursorType::EASTWESTRESIZE
        | CursorType::EASTRESIZE
        | CursorType::WESTRESIZE
        | CursorType::COLUMNRESIZE => Cursor::ResizeHorizontal,
        CursorType::NORTHSOUTHRESIZE
        | CursorType::NORTHRESIZE
        | CursorType::SOUTHRESIZE
        | CursorType::ROWRESIZE => Cursor::ResizeVertical,
        CursorType::NORTHWESTSOUTHEASTRESIZE
        | CursorType::NORTHWESTRESIZE
        | CursorType::SOUTHEASTRESIZE => Cursor::ResizeDiagonal,
        CursorType::NORTHEASTSOUTHWESTRESIZE
        | CursorType::NORTHEASTRESIZE
        | CursorType::SOUTHWESTRESIZE => Cursor::ResizeAntiDiagonal,
        CursorType::NOTALLOWED => Cursor::NotAllowed,
        CursorType::WAIT => Cursor::Wait,
        CursorType::PROGRESS => Cursor::Progress,
        CursorType::HELP => Cursor::Help,
        CursorType::NONE => Cursor::None,
        _ => Cursor::Arrow,
    }
}

wrap_context_menu_handler! {
    pub struct MenuBuilder {
        page: Shared,
    }

    impl ContextMenuHandler {
        /// Chromium's own menu can't show without a window: emptied, and the app told where
        /// it was asked for, to show its own.
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(model) = model {
                model.clear();
            }
            let Some(params) = params else { return };
            let page = &self.page.0;
            let selection = !text_of(&params.selection_text()).is_empty();
            let menu = web_engine_protocol::PageMenu {
                x: params.xcoord(),
                y: params.ycoord(),
                selection,
                editable: params.is_editable() == 1,
            };
            page.send(Event::ContextMenu { page: page.key.page, menu });
        }
    }
}

wrap_accessibility_handler! {
    pub struct AccessBuilder {
        page: Shared,
    }

    impl AccessibilityHandler {
        fn on_accessibility_tree_change(&self, value: Option<&mut Value>) {
            let Some(value) = value else { return };
            self.page.0.access_changed(|tree| tree.apply(&crate::access::to_json(value)));
        }

        /// Nodes moved by the layout alone (the page resized, a block above them grew).
        fn on_accessibility_location_change(&self, value: Option<&mut Value>) {
            let Some(value) = value else { return };
            self.page.0.access_changed(|tree| tree.moved(&crate::access::to_json(value)));
        }
    }
}
