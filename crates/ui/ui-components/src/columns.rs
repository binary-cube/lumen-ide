//! A table's columns fitted to its width by priority: each column has one (0: it never
//! leaves — the name), a width — at least as wide as the widest text it may show, so the
//! numbers don't dance from one second to the next ([`ColumnSpec::reserve`]) — or a range
//! for a flexible one, and what the user chose of it ([`Choice`]: as its priority says,
//! pinned — it stays and pushes others out —, or hidden — never shown). When they don't all
//! fit, the column with the highest priority leaves first, then the next; the pinned ones
//! only once nothing else can; the one the table is sorted by never (it takes the place of
//! the last one shown); then what left and fits again in the room left comes back, the
//! most wanted first. What's left over widens the flexible columns (all but the first up
//! to their most, then the first takes the rest).
//!
//! The user may size a column by dragging its edge ([`ColumnSpec::sized`]): it keeps that
//! width (never under its least the user may drag it to, [`ColumnSpec::min_width`], nor its
//! widest text) and no longer grows; once the first column is sized, the last one shown
//! takes what's left. Its width stands like its spec's: the others leave for it by their
//! priority; only when nothing else can leave does it give way (down to its least) rather
//! than the first column being cut, and it gets its width back once there's room again. The drag itself
//! is [`ColumnFit::edge_at`] (which edge is under the mouse) and [`ColumnFit::dragged`]
//! (the width it makes, between the column's least and what the others leave).
//!
//! Pure and cheap: its user keeps the result for its width and state ([`kernel::Memo`]).

/// What the user chose of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Choice {
    /// As its priority says (it leaves when there's no room).
    #[default]
    Auto,
    /// Kept (it pushes others out).
    Pinned,
    /// Never shown (unless the table is sorted by it).
    Hidden,
}

/// A column of the table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnSpec<K> {
    pub id: K,
    /// 0: never leaves; else the higher, the sooner it leaves.
    pub priority: u8,
    /// Its width (a flexible one's least).
    pub width: f32,
    /// A flexible column's most (`width` for a fixed one; `f32::INFINITY`: any).
    pub max_width: f32,
    /// The widest text it may show (with its padding): it's never narrower.
    pub reserve: f32,
    pub choice: Choice,
    /// The least width the user may drag it to (`width` unless said).
    pub min_width: f32,
    /// The width the user gave it (by dragging its edge), if any.
    pub set: Option<f32>,
}

impl<K> ColumnSpec<K> {
    pub fn new(id: K, priority: u8, width: f32) -> Self {
        Self {
            id,
            priority,
            width,
            max_width: width,
            reserve: 0.0,
            choice: Choice::Auto,
            min_width: width,
            set: None,
        }
    }

    /// Growing up to `max_width` with what's left over.
    pub fn flexible(mut self, max_width: f32) -> Self {
        self.max_width = max_width.max(self.width);
        self
    }

    pub fn reserving(mut self, reserve: f32) -> Self {
        self.reserve = reserve;
        self
    }

    pub fn chosen(mut self, choice: Choice) -> Self {
        self.choice = choice;
        self
    }

    /// The least width the user may drag it to.
    pub fn min(mut self, min_width: f32) -> Self {
        self.min_width = min_width.max(0.0);
        self
    }

    /// The width the user gave it (`None`: as its spec says).
    pub fn sized(mut self, width: Option<f32>) -> Self {
        self.set = width.filter(|w| w.is_finite() && *w > 0.0);
        self
    }

    /// The narrowest the user may make it: its least, never under its widest text.
    pub fn floor(&self) -> f32 {
        self.min_width.max(self.reserve)
    }

    /// Its least width (its own or the one the user gave it, never under its widest text's).
    pub fn least(&self) -> f32 {
        match self.set {
            Some(set) => set.max(self.floor()),
            None => self.width.max(self.reserve),
        }
    }
}

/// Where the columns go.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnFit<K> {
    /// Each column shown, in the table's order: its left edge and width.
    pub shown: Vec<(K, f32, f32)>,
    /// The ones that left for want of room (the chooser says "doesn't fit").
    pub dropped: Vec<K>,
    /// The ones the user hid.
    pub hidden: Vec<K>,
    /// Even what can't leave is wider than the room (the first column is cut).
    pub cramped: bool,
    /// Each shown column's range for a drag of its edge, in `shown`'s order: the narrowest
    /// it may be, and the widest the others leave it.
    pub limits: Vec<(f32, f32)>,
    /// The gap between two columns (their edge is in its middle).
    pub gap: f32,
}

impl<K> Default for ColumnFit<K> {
    fn default() -> Self {
        Self {
            shown: Vec::new(),
            dropped: Vec::new(),
            hidden: Vec::new(),
            cramped: false,
            limits: Vec::new(),
            gap: 0.0,
        }
    }
}

impl<K: Copy + PartialEq> ColumnFit<K> {
    /// Column `id`'s left edge and width, when shown.
    pub fn get(&self, id: K) -> Option<(f32, f32)> {
        self.shown.iter().find(|(k, ..)| *k == id).map(|&(_, x, w)| (x, w))
    }

    pub fn shows(&self, id: K) -> bool {
        self.get(id).is_some()
    }

    /// The right edge of the last column shown.
    pub fn end(&self) -> f32 {
        self.shown.last().map_or(0.0, |&(_, x, w)| x + w)
    }

    /// Each edge that can be dragged: the column on its left and the edge's x (in the gap's
    /// middle). The last column has none (it reaches the table's end).
    pub fn edges(&self) -> impl Iterator<Item = (K, f32)> + '_ {
        let n = self.shown.len().saturating_sub(1);
        self.shown[..n].iter().map(|&(k, x, w)| (k, x + w + self.gap / 2.0))
    }

    /// The column whose edge is within `grip` of `x`.
    pub fn edge_at(&self, x: f32, grip: f32) -> Option<K> {
        self.edges().find(|&(_, e)| (x - e).abs() <= grip).map(|(k, _)| k)
    }

    /// Column `id`'s width when its edge, pressed while it was `from` wide, has moved by
    /// `dx`: between its narrowest and what the others leave it (`None`: not shown).
    pub fn dragged(&self, id: K, from: f32, dx: f32) -> Option<f32> {
        let i = self.shown.iter().position(|(k, ..)| *k == id)?;
        let (least, most) = self.limits.get(i).copied()?;
        Some((from + dx).min(most).max(least).round())
    }
}

/// `columns` fitted from `x0` in `width`, `gap` between two, the table sorted by `sorted`
/// (see the module's doc).
pub fn fit<K: Copy + PartialEq>(
    columns: &[ColumnSpec<K>],
    x0: f32,
    width: f32,
    gap: f32,
    sorted: Option<K>,
) -> ColumnFit<K> {
    let mut out = ColumnFit { gap, ..ColumnFit::default() };
    let is_sorted = |c: &ColumnSpec<K>| sorted == Some(c.id);
    let mut kept: Vec<bool> = columns
        .iter()
        .map(|c| {
            let hidden = c.choice == Choice::Hidden && c.priority > 0 && !is_sorted(c);
            if hidden {
                out.hidden.push(c.id);
            }
            !hidden
        })
        .collect();
    let needed = |kept: &[bool], leasts: &[f32]| {
        let (sum, count) = leasts
            .iter()
            .zip(kept)
            .filter(|(_, k)| **k)
            .fold((0.0, 0usize), |(s, n), (w, _)| (s + w, n + 1));
        sum + gap * count.saturating_sub(1) as f32
    };
    // Each one's least: a sized one's its width (the user's choice stands; the others
    // leave for it by their priority).
    let mut leasts: Vec<f32> = columns.iter().map(ColumnSpec::least).collect();
    // The next to leave: the highest priority not pinned, then the pinned ones; never the
    // first rank or the sorted one; equal priorities, the last listed first.
    let next = |kept: &[bool]| {
        let candidates = |pinned: bool| {
            columns
                .iter()
                .enumerate()
                .filter(|&(i, c)| {
                    kept[i]
                        && c.priority > 0
                        && !is_sorted(c)
                        && (c.choice == Choice::Pinned) == pinned
                })
                .max_by_key(|&(i, c)| (c.priority, i))
                .map(|(i, _)| i)
        };
        candidates(false).or_else(|| candidates(true))
    };
    while needed(&kept, &leasts) > width + 0.01 {
        let Some(i) = next(&kept) else {
            out.cramped = true;
            break;
        };
        kept[i] = false;
        out.dropped.push(columns[i].id);
    }
    // What left on the way and fits again in the room left comes back, the most wanted
    // first (a narrow column where a wider, more wanted one didn't fit): none hidden for
    // nothing.
    if !out.cramped {
        let mut gone: Vec<usize> =
            (0..columns.len()).filter(|&i| out.dropped.contains(&columns[i].id)).collect();
        gone.sort_by_key(|&i| (columns[i].priority, i));
        for i in gone {
            kept[i] = true;
            if needed(&kept, &leasts) > width + 0.01 {
                kept[i] = false;
            }
        }
        out.dropped.retain(|id| columns.iter().zip(&kept).any(|(c, k)| c.id == *id && !k));
    }
    // Nothing else can leave: the sized ones give way (the last first, down to their
    // floor) before the first column is cut.
    if out.cramped {
        for i in (0..columns.len()).rev() {
            let excess = needed(&kept, &leasts) - width;
            if excess <= 0.01 {
                break;
            }
            if kept[i] && columns[i].set.is_some() {
                leasts[i] -= (leasts[i] - columns[i].floor()).max(0.0).min(excess);
            }
        }
        out.cramped = needed(&kept, &leasts) > width + 0.01;
    }
    // Widths: their least, then what's left grows the flexible ones not sized; the rest
    // goes to the first, or to the last shown once the first is sized.
    let mut widths = leasts.clone();
    let slack = (width - needed(&kept, &leasts)).max(0.0);
    let mut spare = slack;
    for (i, c) in columns.iter().enumerate().skip(1) {
        if kept[i] && c.set.is_none() && c.max_width > c.least() {
            let grow = (c.max_width - c.least()).min(spare);
            widths[i] += grow;
            spare -= grow;
        }
    }
    if let Some(first) = columns.first()
        && kept[0]
    {
        if out.cramped {
            // What can't leave, wider than the room: the first column gives it up.
            let others: f32 =
                (1..columns.len()).filter(|&i| kept[i]).map(|i| widths[i] + gap).sum();
            widths[0] = (width - others).max(0.0);
        } else if first.set.is_none() {
            widths[0] += spare.min((first.max_width - first.least()).max(0.0));
        } else if let Some(last) = (0..columns.len()).rev().find(|&i| kept[i]) {
            widths[last] += spare;
        }
    }
    let mut x = x0.round();
    for (i, c) in columns.iter().enumerate() {
        if kept[i] {
            let w = widths[i].floor();
            out.shown.push((c.id, x, w));
            let least = c.floor().min(w);
            out.limits.push((least, (leasts[i] + slack).max(least).floor()));
            x += w + gap;
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/columns.rs"]
mod tests;
