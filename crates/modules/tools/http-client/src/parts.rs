//! What the views' parts share here: the keys given to the first part that takes them.

use ui_components::control::{Control, Controls};

/// The keys to the first part that takes them (a list open over a field, a name being typed
/// over a table), else to none.
pub(crate) fn keys_to_first(controls: &mut Controls, parts: &mut [&mut dyn Control]) {
    let first = parts.iter().position(|c| c.focusable());
    controls.focus(parts, first);
}
