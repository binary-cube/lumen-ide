//! Which rows the grid shows: a page of them (`1–500 of N`, moved with `|< < > >|`), or all
//! of them (scrolled, read as they come into view).

/// The page sizes offered (and "all").
pub const PAGE_SIZES: [u32; 4] = [10, 100, 500, 1000];

/// Above this many rows, showing them all is asked about first (a warning in the menu).
pub const LARGE: u64 = 100_000;

/// The rows a page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paging {
    /// Rows per page; `None`: all of them.
    pub size: Option<u32>,
    /// The page's first row (0 with all of them).
    pub start: u64,
}

impl Paging {
    pub fn new(size: Option<u32>) -> Self {
        Self { size: size.map(|s| s.max(1)), start: 0 }
    }

    /// The rows the page holds: `total` known, as many as are left of it; not known, a
    /// whole page (all of them: `loaded`, what's known to be there so far).
    pub fn rows(&self, total: Option<u64>, loaded: u64) -> u64 {
        match (self.size, total) {
            (Some(size), Some(total)) => total.saturating_sub(self.start).min(u64::from(size)),
            (Some(size), None) => u64::from(size),
            (None, Some(total)) => total,
            (None, None) => loaded,
        }
    }

    /// The first and last rows shown (1-based, as `1–500`), `None` when there are none.
    pub fn span(&self, total: Option<u64>, loaded: u64) -> Option<(u64, u64)> {
        let rows = self.rows(total, loaded);
        (rows > 0).then(|| (self.start + 1, self.start + rows))
    }

    pub fn can_back(&self) -> bool {
        self.size.is_some() && self.start > 0
    }

    /// There's a page after this one (`total` not known: as long as this one is full).
    pub fn can_forward(&self, total: Option<u64>, full: bool) -> bool {
        match (self.size, total) {
            (None, _) => false,
            (Some(size), Some(total)) => self.start + u64::from(size) < total,
            (Some(_), None) => full,
        }
    }

    pub fn first(&mut self) {
        self.start = 0;
    }

    pub fn back(&mut self) {
        if let Some(size) = self.size {
            self.start = self.start.saturating_sub(u64::from(size));
        }
    }

    pub fn forward(&mut self) {
        if let Some(size) = self.size {
            self.start += u64::from(size);
        }
    }

    /// The last page (`total` known).
    pub fn last(&mut self, total: u64) {
        if let Some(size) = self.size {
            let size = u64::from(size);
            self.start = total.saturating_sub(1) / size * size;
        }
    }

    /// Another page size: the page holding the row shown first now.
    pub fn resize(&mut self, size: Option<u32>) {
        self.size = size.map(|s| s.max(1));
        self.start = match self.size {
            Some(size) => self.start / u64::from(size) * u64::from(size),
            None => 0,
        };
    }

    /// Back on a page that holds rows, when `total` shrank below it (a filter, a delete).
    pub fn clamp(&mut self, total: u64) {
        if self.start >= total && self.start > 0 {
            self.last(total);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/paging.rs"]
mod tests;
