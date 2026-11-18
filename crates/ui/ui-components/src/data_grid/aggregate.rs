//! What the selected cells add up to, in the grid's bottom bar: how many, and for the
//! numbers among them their sum, average, smallest and largest.

use super::source::CellRef;

/// The selection's figures.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Aggregates {
    /// Cells counted (`NULL` too).
    pub count: u64,
    /// Of them, numbers.
    pub numbers: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    /// Every number was an integer (shown without decimals), summed exactly.
    integers: bool,
    int_sum: i128,
}

impl Aggregates {
    pub fn new() -> Self {
        Self { integers: true, ..Self::default() }
    }

    pub fn add(&mut self, value: &CellRef<'_>) {
        self.count += 1;
        let Some(n) = value.number() else { return };
        if self.numbers == 0 {
            (self.min, self.max) = (n, n);
        } else {
            self.min = self.min.min(n);
            self.max = self.max.max(n);
        }
        self.numbers += 1;
        self.sum += n;
        match value {
            CellRef::Int(i) => self.int_sum += i128::from(*i),
            _ => self.integers = false,
        }
    }

    pub fn average(&self) -> Option<f64> {
        (self.numbers > 0).then(|| self.sum / self.numbers as f64)
    }

    /// The sum as shown (an integers' sum exact).
    pub fn sum_text(&self) -> String {
        if self.integers { self.int_sum.to_string() } else { number(self.sum) }
    }

    /// The smallest and the largest as shown.
    pub fn min_text(&self) -> String {
        number(self.min)
    }

    pub fn max_text(&self) -> String {
        number(self.max)
    }
}

/// A figure as shown: whole numbers without decimals, others to 4 decimals at most
/// (trailing zeros dropped).
pub fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{n:.0}");
    }
    let text = format!("{n:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_owned() } else { text.to_owned() }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/aggregate.rs"]
mod tests;
