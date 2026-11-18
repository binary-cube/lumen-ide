//! What PHP itself provides under the mouse, from our stubs: a built-in function with its
//! signature, a class or a constant, with the version it appeared in (or was removed in).

use code_intelligence::{HoverInfo, HoverNote};

pub fn hover(text: &str, offset: usize) -> Option<HoverInfo> {
    let range = code_intelligence::word_at(text, offset, "$\\")?;
    let word = text[range.clone()].trim_start_matches('$');
    if text[range.clone()].starts_with('$') {
        return None;
    }
    let before = text[..range.start].trim_end();
    // Methods and properties (`->x`, `::x`) and declarations (`function x`) aren't built-ins.
    if before.ends_with("->") || before.ends_with("::") || before.ends_with("function") {
        return None;
    }
    let stubs = crate::stubs::get();
    let notes = |name: &str| {
        let mut notes = Vec::new();
        if let Some(since) = stubs.since(name) {
            notes.push(HoverNote::Since(since));
        }
        if let Some(removed) = stubs.removed(name) {
            notes.push(HoverNote::Removed(removed));
        }
        if let Some(extension) = stubs.extension(name) {
            notes.push(HoverNote::Extension(extension));
        }
        notes
    };
    let name = word.trim_start_matches('\\');
    let call = text[range.end..].trim_start().starts_with('(');
    if call && let Some(f) = stubs.function(name) {
        let code = format!("{}{}", f.name, f.signature());
        return Some(HoverInfo {
            range,
            code,
            notes: notes(name),
            doc: None,
            ..Default::default()
        });
    }
    if stubs.class(name).is_some() {
        return Some(HoverInfo {
            range,
            code: format!("class {name}"),
            notes: notes(name),
            doc: None,
            ..Default::default()
        });
    }
    if stubs.constant(name).is_some() {
        return Some(HoverInfo {
            range,
            code: format!("const {name}"),
            notes: notes(name),
            doc: None,
            ..Default::default()
        });
    }
    None
}

#[cfg(test)]
#[path = "../tests/unit/hover.rs"]
mod tests;
