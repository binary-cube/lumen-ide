//! A three-way merge by line: what each side changed from their common ancestor (the base),
//! as regions — agreed on, changed on one side only (that side taken), changed the same on
//! both, or changed differently on both (a conflict). Then the same as a comparison of the
//! two sides ([`Chunk`]s between ours and theirs) with each difference's starting pick, so
//! a two-sided view composes the result: one side's changes are taken already, the
//! conflicts wait for a choice.

use std::ops::Range;

use crate::{Chunk, Pick, diff_lines};

/// What happened to a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// Nobody changed it.
    Equal,
    /// Only ours changed it.
    Ours,
    /// Only theirs changed it.
    Theirs,
    /// Both changed it the same way.
    Same,
    /// Both changed it, differently.
    Conflict,
}

/// A region: its lines in the base, in ours and in theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub kind: RegionKind,
    pub base: Range<usize>,
    pub ours: Range<usize>,
    pub theirs: Range<usize>,
}

/// A side's change: the base's lines it replaced, by which of its own.
#[derive(Debug, Clone)]
struct Edit {
    base: Range<usize>,
    side: Range<usize>,
    ours: bool,
}

/// The side's changes from `base`.
fn edits<S: AsRef<str>>(base: &[S], side: &[S], ours: bool) -> Vec<Edit> {
    diff_lines(base, side)
        .into_iter()
        .filter_map(|chunk| match chunk {
            Chunk::Change { left, right } => Some(Edit { base: left, side: right, ours }),
            Chunk::Equal { .. } => None,
        })
        .collect()
}

/// The regions of merging `ours` and `theirs`, both made from `base`, in order: together
/// they cover each text whole.
pub fn merge3<S: AsRef<str>>(base: &[S], ours: &[S], theirs: &[S]) -> Vec<Region> {
    let mut all = edits(base, ours, true);
    all.extend(edits(base, theirs, false));
    all.sort_by_key(|e| (e.base.start, e.base.end));
    // The lines each side has more than the base before a base line (its edits so far).
    let (mut shift_ours, mut shift_theirs) = (0isize, 0isize);
    let at = |line: usize, shift: isize| line.saturating_add_signed(shift);
    let mut regions = Vec::new();
    let mut done = 0;
    let mut i = 0;
    while i < all.len() {
        // The edits touching each other (on either side) make one group.
        let mut group = vec![all[i].clone()];
        let mut end = all[i].base.end;
        i += 1;
        while i < all.len() && all[i].base.start <= end && touches(&group, &all[i], end) {
            end = end.max(all[i].base.end);
            group.push(all[i].clone());
            i += 1;
        }
        let start = group[0].base.start;
        // The lines agreed on before it.
        if start > done {
            regions.push(Region {
                kind: RegionKind::Equal,
                base: done..start,
                ours: at(done, shift_ours)..at(start, shift_ours),
                theirs: at(done, shift_theirs)..at(start, shift_theirs),
            });
        }
        let span = |is_ours: bool, shift: isize| -> (Range<usize>, isize, bool) {
            let mine: Vec<&Edit> = group.iter().filter(|e| e.ours == is_ours).collect();
            let grown: isize =
                mine.iter().map(|e| e.side.len() as isize - e.base.len() as isize).sum();
            let range = at(start, shift)..at(end, shift + grown);
            (range, shift + grown, !mine.is_empty())
        };
        let (ours_range, next_ours, ours_changed) = span(true, shift_ours);
        let (theirs_range, next_theirs, theirs_changed) = span(false, shift_theirs);
        let kind = match (ours_changed, theirs_changed) {
            (true, false) => RegionKind::Ours,
            (false, true) => RegionKind::Theirs,
            _ => {
                let same = ours[ours_range.clone()]
                    .iter()
                    .map(AsRef::as_ref)
                    .eq(theirs[theirs_range.clone()].iter().map(AsRef::as_ref));
                if same { RegionKind::Same } else { RegionKind::Conflict }
            }
        };
        regions.push(Region { kind, base: start..end, ours: ours_range, theirs: theirs_range });
        (shift_ours, shift_theirs, done) = (next_ours, next_theirs, end);
    }
    if done < base.len() || regions.is_empty() {
        regions.push(Region {
            kind: RegionKind::Equal,
            base: done..base.len(),
            ours: at(done, shift_ours)..ours.len(),
            theirs: at(done, shift_theirs)..theirs.len(),
        });
    }
    regions.retain(|r| !(r.kind == RegionKind::Equal && r.base.is_empty() && r.ours.is_empty()));
    regions
}

/// `next` joins the group ending at `end`: it overlaps it, or starts where it ends with an
/// edit of the other side involved (lines inserted at the same place, or right after the
/// other side's change: git calls that a conflict too).
fn touches(group: &[Edit], next: &Edit, end: usize) -> bool {
    next.base.start < end
        || group.iter().any(|e| e.ours != next.ours && e.base.end == next.base.start)
}

/// The merge as a comparison of ours (left) with theirs (right): the chunks between them
/// and, for each difference, the pick it starts with and whether it's a conflict (its pick
/// to be chosen: it starts with neither).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MergeChunks {
    pub chunks: Vec<Chunk>,
    pub picks: Vec<Pick>,
    pub conflicts: Vec<bool>,
}

impl MergeChunks {
    pub fn of(regions: &[Region]) -> Self {
        let mut merged = Self::default();
        for region in regions {
            let (ours, theirs) = (region.ours.clone(), region.theirs.clone());
            let pick = match region.kind {
                RegionKind::Equal | RegionKind::Same => {
                    merged.chunks.push(Chunk::Equal { left: ours, right: theirs });
                    continue;
                }
                RegionKind::Ours => Pick::Left,
                RegionKind::Theirs => Pick::Right,
                RegionKind::Conflict => Pick::None,
            };
            merged.chunks.push(Chunk::Change { left: ours, right: theirs });
            merged.picks.push(pick);
            merged.conflicts.push(region.kind == RegionKind::Conflict);
        }
        merged
    }

    /// How many conflicts there are.
    pub fn conflict_count(&self) -> usize {
        self.conflicts.iter().filter(|c| **c).count()
    }
}
