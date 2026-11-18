//! The log's graph: which column ("lane") each commit sits in and the lines joining it to
//! its children above and its parents below — built row by row, so a log read in pages
//! continues the same graph.
//!
//! Each lane waits for a commit (the next one on that line of history). A commit takes the
//! lane waiting for it (lanes also waiting for it join it: a branch point); its first
//! parent continues in that lane, other parents (a merge) get their own lanes. Lanes keep
//! their column while they live, so lines run straight.

/// One row of the graph.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphRow {
    /// The commit's column.
    pub column: usize,
    /// Lanes running through the row (not this commit's), by column.
    pub through: Vec<usize>,
    /// Lanes coming from above into the commit (its column too, unless it starts a line).
    pub from_above: Vec<usize>,
    /// Lanes leaving the commit downward, toward its parents (its column first).
    pub to_below: Vec<usize>,
    /// How many columns the row uses.
    pub width: usize,
}

/// Builds the graph as commits arrive, newest first.
#[derive(Debug, Clone, Default)]
pub struct GraphBuilder {
    /// What each lane waits for (`None`: free).
    lanes: Vec<Option<String>>,
}

impl GraphBuilder {
    /// The row of commit `id` with `parents`.
    pub fn push(&mut self, id: &str, parents: &[String]) -> GraphRow {
        let waiting: Vec<usize> =
            (0..self.lanes.len()).filter(|&i| self.lanes[i].as_deref() == Some(id)).collect();
        let column = match waiting.first() {
            Some(&c) => c,
            None => self.free_lane(),
        };
        let through: Vec<usize> = (0..self.lanes.len())
            .filter(|&i| self.lanes[i].is_some() && !waiting.contains(&i))
            .collect();
        // The lanes that waited for it end here.
        for &i in &waiting {
            self.lanes[i] = None;
        }
        let mut to_below = Vec::new();
        for (n, parent) in parents.iter().enumerate() {
            let lane = if n == 0 {
                column
            } else if let Some(i) = self.lanes.iter().position(|l| l.as_deref() == Some(parent)) {
                // Another lane already goes to that parent: join it.
                i
            } else {
                self.free_lane()
            };
            if self.lanes.len() <= lane {
                self.lanes.resize(lane + 1, None);
            }
            self.lanes[lane] = Some(parent.clone());
            if !to_below.contains(&lane) {
                to_below.push(lane);
            }
        }
        while self.lanes.last().is_some_and(Option::is_none) {
            self.lanes.pop();
        }
        let width = through
            .iter()
            .chain(&waiting)
            .chain(&to_below)
            .copied()
            .chain([column])
            .max()
            .map_or(1, |m| m + 1);
        GraphRow { column, through, from_above: waiting, to_below, width }
    }

    /// The first free lane (a new one at the end if none is).
    fn free_lane(&mut self) -> usize {
        match self.lanes.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.lanes.push(None);
                self.lanes.len() - 1
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/graph.rs"]
mod tests;
