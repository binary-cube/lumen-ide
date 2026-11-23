//! The rows a chart is made from, as whoever holds them lends them: a query's result, a
//! table, a recording, a file. Read through [`Tabular`], never copied whole: a chart keeps
//! only what it prepared from them.

use std::borrow::Cow;

/// What a column holds, for which roles it can take (a Y must be a number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Number,
    /// A moment: read as seconds since the epoch ([`Tabular::number`]).
    Time,
    Text,
}

impl ColumnKind {
    /// Its values can be measured (on a Y, a scatter's X, a histogram).
    pub fn measurable(self) -> bool {
        matches!(self, ColumnKind::Number | ColumnKind::Time)
    }
}

/// A column: its name (how a spec names it) and what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
}

impl Column {
    pub fn new(name: impl Into<String>, kind: ColumnKind) -> Self {
        Self { name: name.into(), kind }
    }
}

/// Rows lent to a chart.
pub trait Tabular {
    fn columns(&self) -> &[Column];

    /// How many rows there are to read.
    fn rows(&self) -> usize;

    /// Cell (`row`, `col`) as a number (a moment: seconds since the epoch); `None`: empty,
    /// or not a number.
    fn number(&self, row: usize, col: usize) -> Option<f64>;

    /// Cell (`row`, `col`) as it reads (a category's name); `None`: empty.
    fn text(&self, row: usize, col: usize) -> Option<Cow<'_, str>>;

    /// The column named `name`.
    fn column(&self, name: &str) -> Option<usize> {
        self.columns().iter().position(|c| c.name == name)
    }
}

/// A moment written as databases and files write it — `2026-11-19`, `2026-11-19 09:09:11`,
/// `2026-11-19T09:09:11.074537Z`, `…+02:00` — as seconds since the epoch (UTC; one with no
/// offset read as UTC). `None`: not a moment.
pub fn moment(text: &str) -> Option<f64> {
    let t = text.trim();
    let num = |s: &str| -> Option<i64> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
    };
    let date = t.get(..10)?;
    let mut parts = date.split('-');
    let (y, m, d) = (num(parts.next()?)?, num(parts.next()?)?, num(parts.next()?)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || date.len() != 10 {
        return None;
    }
    let days = crate::scale::days_from_civil(y, m as u32, d as u32);
    let mut seconds = days as f64 * 86_400.0;
    let rest = t.get(10..).unwrap_or("");
    let rest = rest.strip_prefix(['T', ' ']).unwrap_or(rest);
    if rest.is_empty() {
        return Some(seconds);
    }
    // The time, then an offset (Z, +hh:mm, -hh:mm, +hh).
    let cut = rest
        .find(['Z', 'z', '+'])
        .or_else(|| rest.get(1..).and_then(|r| r.find('-')).map(|i| i + 1));
    let (clock, offset) = match cut {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut hms = clock.split(':');
    let h = num(hms.next()?)?;
    let min = num(hms.next().unwrap_or("0"))?;
    let sec: f64 = hms.next().map_or(Some(0.0), |s| s.parse().ok())?;
    if h > 24 || min > 59 || !(0.0..61.0).contains(&sec) {
        return None;
    }
    seconds += (h * 3600 + min * 60) as f64 + sec;
    if let Some(sign) = offset.chars().next().filter(|c| *c == '+' || *c == '-') {
        let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
        let (oh, om) = (num(digits.get(..2)?)?, num(digits.get(2..4).unwrap_or("0"))?);
        let shift = (oh * 3600 + om * 60) as f64;
        seconds += if sign == '+' { -shift } else { shift };
    }
    Some(seconds)
}

/// Rows kept in memory: a test's, a small listing's.
#[derive(Debug, Clone, Default)]
pub struct Rows {
    pub columns: Vec<Column>,
    /// Each row's cells, as text (numbers parsed when asked).
    pub cells: Vec<Vec<Option<String>>>,
}

impl Rows {
    pub fn new(columns: Vec<Column>) -> Self {
        Self { columns, cells: Vec::new() }
    }

    /// A row added, its cells in the columns' order.
    pub fn push<S: ToString>(&mut self, row: impl IntoIterator<Item = Option<S>>) {
        self.cells.push(row.into_iter().map(|c| c.map(|v| v.to_string())).collect());
    }
}

impl Tabular for Rows {
    fn columns(&self) -> &[Column] {
        &self.columns
    }

    fn rows(&self) -> usize {
        self.cells.len()
    }

    fn number(&self, row: usize, col: usize) -> Option<f64> {
        let cell = self.cells.get(row)?.get(col)?.as_deref()?;
        cell.trim().parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn text(&self, row: usize, col: usize) -> Option<Cow<'_, str>> {
        self.cells.get(row)?.get(col)?.as_deref().map(Cow::Borrowed)
    }
}

#[cfg(test)]
#[path = "../tests/unit/data.rs"]
mod tests;
