//! Where things are on screen, by an id (a hint points at them): the app, its views and
//! panels report a part's rect as they draw it (`report`), and what's drawn over them later
//! in the same frame finds it (`find`). Each window's frame starts afresh (`begin`): a part
//! not drawn this frame isn't found. Kept on the drawing thread in one small list, reused
//! from frame to frame (nothing allocated once it's grown).

use std::cell::RefCell;

use kernel::Rect;

/// A part's name: a kind of part (`editor.tab`) and, among several of it, which one (a hash
/// of its own key; 0: the only one).
type Name = (&'static str, u64);

thread_local! {
    static ANCHORS: RefCell<Vec<(Name, Rect)>> = const { RefCell::new(Vec::new()) };
}

/// `key`'s hash (the same in every run: nothing kept between them).
fn hashed(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() | 1
}

/// A window's frame starts: what was reported before is let go.
pub fn begin() {
    ANCHORS.with(|a| a.borrow_mut().clear());
}

/// Part `id` is at `rect` (physical pixels) in this frame (the latest report wins).
pub fn report(id: &'static str, rect: Rect) {
    report_name((id, 0), rect);
}

/// One of the parts of kind `id` (a tab), known by its own `key` (its name), is at `rect`
/// this frame. Nothing is allocated: the key is hashed.
pub fn report_keyed(id: &'static str, key: &str, rect: Rect) {
    report_name((id, hashed(key)), rect);
}

fn report_name(name: Name, rect: Rect) {
    ANCHORS.with(|a| {
        let mut anchors = a.borrow_mut();
        match anchors.iter_mut().find(|(n, _)| *n == name) {
            Some(at) => at.1 = rect,
            None => anchors.push((name, rect)),
        }
    });
}

/// Where part `id` was drawn in this frame, if it was.
pub fn find(id: &str) -> Option<Rect> {
    find_name(id, 0)
}

/// Where the part of kind `id` known by `key` was drawn in this frame, if it was.
pub fn find_keyed(id: &str, key: &str) -> Option<Rect> {
    find_name(id, hashed(key))
}

fn find_name(id: &str, key: u64) -> Option<Rect> {
    ANCHORS.with(|a| a.borrow().iter().find(|((i, k), _)| *i == id && *k == key).map(|(_, r)| *r))
}

#[cfg(test)]
#[path = "../tests/unit/anchors.rs"]
mod tests;
