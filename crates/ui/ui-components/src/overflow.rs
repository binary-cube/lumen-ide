//! A bar whose items don't all fit: what's placed, what goes into its "…" menu. Items stand
//! on the left (from the bar's start) or on the right (ending at its end, the "…" after
//! them); each has a width — a flexible one (a filter field) shrinks to its least first —
//! and its place in the order they go when there's no room ([`OverflowItem::collapse`];
//! `None`: never). They go one by one in that order until the rest fits; then what went on
//! the way and fits again in the room left comes back, the most wanted first (a narrow one
//! where a wide one didn't fit), so nothing is hidden for nothing. The menu lists them in
//! their own order ([`OverflowItem::menu`]).
//!
//! Pure and cheap (items × steps): its user keeps the result for its width and state
//! ([`kernel::Memo`]), as the data grid's toolbar does.

/// Where an item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// An item of the bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverflowItem<T> {
    pub id: T,
    /// Its width when there's room.
    pub width: f32,
    /// The least it shrinks to (its `width` for a fixed one).
    pub min_width: f32,
    pub side: Side,
    /// When it goes: the lower first (`None`: never, it stays — cut at worst).
    pub collapse: Option<u8>,
    /// Its place in the "…" menu.
    pub menu: u8,
}

impl<T> OverflowItem<T> {
    /// A fixed item.
    pub fn new(id: T, width: f32, side: Side, collapse: Option<u8>, menu: u8) -> Self {
        Self { id, width, min_width: width, side, collapse, menu }
    }

    /// Shrinking down to `min_width` before anything goes.
    pub fn flexible(mut self, min_width: f32) -> Self {
        self.min_width = min_width.min(self.width);
        self
    }
}

/// The bar and its items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverflowSpec<'a, T> {
    pub min_x: f32,
    pub max_x: f32,
    /// From the bar's ends.
    pub pad: f32,
    /// Between two items.
    pub gap: f32,
    /// The "…" button's width.
    pub more_width: f32,
    pub items: &'a [OverflowItem<T>],
}

/// Where everything goes.
#[derive(Debug, Clone, PartialEq)]
pub struct OverflowLayout<T> {
    /// Each item shown: its left edge and width (a flexible one as wide as the room lets it).
    pub placed: Vec<(T, f32, f32)>,
    /// What's in the "…" menu, in the menu's order.
    pub collapsed: Vec<T>,
    /// The "…" button's left edge and width, when something went into it.
    pub more: Option<(f32, f32)>,
    /// Everything shown fits (else even the items that never go are wider than the bar).
    pub fits: bool,
}

impl<T> Default for OverflowLayout<T> {
    fn default() -> Self {
        Self { placed: Vec::new(), collapsed: Vec::new(), more: None, fits: true }
    }
}

impl<T: Copy + PartialEq> OverflowLayout<T> {
    /// Item `id`'s left edge and width, when shown.
    pub fn get(&self, id: T) -> Option<(f32, f32)> {
        self.placed.iter().find(|(t, ..)| *t == id).map(|&(_, x, w)| (x, w))
    }

    pub fn shows(&self, id: T) -> bool {
        self.get(id).is_some()
    }
}

/// The room the items not in `hidden` need at least (their least widths, the gaps, the
/// "…" when something is hidden).
fn needed<T: Copy + PartialEq>(spec: &OverflowSpec<'_, T>, hidden: &[usize]) -> f32 {
    let mut total = 2.0 * spec.pad;
    let mut count = 0usize;
    for (i, item) in spec.items.iter().enumerate() {
        if !hidden.contains(&i) {
            total += item.min_width;
            count += 1;
        }
    }
    if !hidden.is_empty() {
        total += spec.more_width;
        count += 1;
    }
    total + spec.gap * count.saturating_sub(1) as f32
}

/// The layout of `spec` (see the module's doc).
pub fn fit<T: Copy + PartialEq>(spec: &OverflowSpec<'_, T>) -> OverflowLayout<T> {
    let room = (spec.max_x - spec.min_x).max(0.0);
    let fits = |hidden: &[usize]| needed(spec, hidden) <= room + 0.01;
    // The order they go in: by their step, then as listed.
    let mut order: Vec<usize> =
        (0..spec.items.len()).filter(|&i| spec.items[i].collapse.is_some()).collect();
    order.sort_by_key(|&i| (spec.items[i].collapse, i));
    let mut hidden: Vec<usize> = Vec::new();
    let mut taken = Vec::new();
    let mut steps = order.into_iter();
    let mut all_fit = fits(&hidden);
    while !all_fit {
        let Some(i) = steps.next() else { break };
        hidden.push(i);
        taken.push(i);
        all_fit = fits(&hidden);
    }
    if all_fit {
        // What went on the way and fits again comes back, the most wanted first.
        for &i in taken.iter().rev() {
            let fewer: Vec<usize> = hidden.iter().copied().filter(|&h| h != i).collect();
            if fits(&fewer) {
                hidden = fewer;
            }
        }
    }
    place(spec, &hidden, all_fit)
}

fn place<T: Copy + PartialEq>(
    spec: &OverflowSpec<'_, T>,
    hidden: &[usize],
    fits: bool,
) -> OverflowLayout<T> {
    let room = (spec.max_x - spec.min_x).max(0.0);
    // What the flexible items may grow by, the first first.
    let mut spare = (room - needed(spec, hidden)).max(0.0);
    let mut widths: Vec<f32> = spec.items.iter().map(|i| i.min_width).collect();
    for (i, item) in spec.items.iter().enumerate() {
        if hidden.contains(&i) {
            continue;
        }
        let grow = (item.width - item.min_width).min(spare);
        widths[i] += grow;
        spare -= grow;
    }
    // Whole pixels: two neighbors never overlap by a rounding.
    for w in &mut widths {
        *w = w.floor();
    }
    let mut out = OverflowLayout { fits, ..OverflowLayout::default() };
    let mut x = (spec.min_x + spec.pad).round();
    for (i, item) in spec.items.iter().enumerate() {
        if item.side == Side::Left && !hidden.contains(&i) {
            out.placed.push((item.id, x, widths[i]));
            x += widths[i] + spec.gap;
        }
    }
    let mut right = (spec.max_x - spec.pad).round();
    if !hidden.is_empty() {
        right -= spec.more_width;
        out.more = Some((right, spec.more_width));
        right -= spec.gap;
    }
    for (i, item) in spec.items.iter().enumerate().rev() {
        if item.side == Side::Right && !hidden.contains(&i) {
            right -= widths[i];
            out.placed.push((item.id, right, widths[i]));
            right -= spec.gap;
        }
    }
    let mut collapsed: Vec<usize> = hidden.to_vec();
    collapsed.sort_by_key(|&i| (spec.items[i].menu, i));
    out.collapsed = collapsed.into_iter().map(|i| spec.items[i].id).collect();
    out
}

#[cfg(test)]
#[path = "../tests/unit/overflow.rs"]
mod tests;
