//! The map between the source's lines and the laid-out document's height, for scrolling
//! the text and its preview together (Split): a point where each block's text starts (its
//! first source line at the top of its first run), the document's start and end; between
//! two points, the lines are spread by height — a long block scrolls smoothly, an image or a
//! diagram taller than its lines takes them in proportion. Built with the layout, looked up
//! by binary search: nothing laid out again to scroll.

use crate::select::TextRun;

/// Points (y, source line), both never decreasing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineMap {
    points: Vec<(f32, f32)>,
}

impl LineMap {
    /// The map of a layout `height` tall whose text is `runs`, of `source`.
    pub fn build(runs: &[TextRun], source: &str, height: f32) -> Self {
        let starts: Vec<usize> =
            std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let line_of = |byte: usize| (starts.partition_point(|&s| s <= byte) - 1) as f32;
        let lines = if source.ends_with('\n') { starts.len() - 1 } else { starts.len() } as f32;
        let mut points = vec![(0.0, 0.0)];
        let mut last_src = usize::MAX..usize::MAX;
        for r in runs {
            if r.src.is_empty() || r.src == last_src {
                continue;
            }
            last_src = r.src.clone();
            points.push((r.rect.min_y(), line_of(r.src.start)));
        }
        points.push((height.max(0.0), lines));
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        // A block shown away from its source (a footnote at the end): no step back.
        let mut most: f32 = 0.0;
        for p in &mut points {
            most = most.max(p.1.min(lines));
            p.1 = most;
        }
        points.dedup_by(|b, a| (a.0 - b.0).abs() < 0.5);
        Self { points }
    }

    /// The source line (fractional) at height `y`.
    pub fn line_at(&self, y: f32) -> f32 {
        let p = &self.points;
        let Some(&(_, last)) = p.last() else { return 0.0 };
        let i = p.partition_point(|q| q.0 <= y);
        if i == 0 {
            return 0.0;
        }
        if i == p.len() {
            return last;
        }
        let ((y0, l0), (y1, l1)) = (p[i - 1], p[i]);
        l0 + (l1 - l0) * ((y - y0) / (y1 - y0).max(f32::EPSILON)).clamp(0.0, 1.0)
    }

    /// The height where source line `line` (fractional) is.
    pub fn y_of(&self, line: f32) -> f32 {
        let p = &self.points;
        let Some(&(last, _)) = p.last() else { return 0.0 };
        let i = p.partition_point(|q| q.1 < line);
        if i == 0 {
            return 0.0;
        }
        if i == p.len() {
            return last;
        }
        let ((y0, l0), (y1, l1)) = (p[i - 1], p[i]);
        y0 + (y1 - y0) * ((line - l0) / (l1 - l0).max(f32::EPSILON)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
#[path = "../tests/unit/sync.rs"]
mod tests;
