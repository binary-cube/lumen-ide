//! A column's kind from its values: a number when every value is one (commas between
//! thousands when every comma is followed by three digits, `1,204`; else a decimal comma
//! when every value with a fraction writes it so), a moment when every one is, a text
//! otherwise. An empty cell doesn't decide.

use charts_model::ColumnKind;

use crate::Cell;

/// How a column writes its numbers' marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Marks {
    /// A dot before the fraction (`1204.5`, `1 204.5`).
    #[default]
    Plain,
    /// Commas between thousands (`1,204.5`).
    Thousands,
    /// A comma before the fraction (`1,5`, `1.204,5`).
    DecimalComma,
}

/// The kinds of `count` columns of `rows`, and how each writes its numbers.
pub fn kinds(count: usize, rows: &[Vec<Cell>]) -> (Vec<ColumnKind>, Vec<Marks>) {
    (0..count)
        .map(|c| {
            let values = || rows.iter().filter_map(|r| r.get(c)).filter(|v| **v != Cell::Empty);
            if values().next().is_none() {
                return (ColumnKind::Text, Marks::Plain);
            }
            let texts = || {
                values().filter_map(|v| if let Cell::Text(t) = v { Some(t.as_str()) } else { None })
            };
            if texts().all(|t| number(t, Marks::Plain).is_some()) {
                return (ColumnKind::Number, Marks::Plain);
            }
            // Commas between thousands when every comma is one (`1,204`, `12,500.75`): a
            // value with another (`1,5`) proves a decimal comma instead.
            if texts().all(|t| number(t, Marks::Thousands).is_some()) {
                return (ColumnKind::Number, Marks::Thousands);
            }
            // A decimal comma only where no value writes a lone dot (`2.5` among `1,5`).
            if texts().all(|t| !t.contains('.') || t.contains(','))
                && texts().all(|t| number(t, Marks::DecimalComma).is_some())
            {
                return (ColumnKind::Number, Marks::DecimalComma);
            }
            if values().all(|v| matches!(v, Cell::Text(t) if charts_model::moment(t).is_some())) {
                return (ColumnKind::Time, Marks::Plain);
            }
            (ColumnKind::Text, Marks::Plain)
        })
        .unzip()
}

/// `t` as a number: plain (`1204`, `-0.5`, `1e3`), with spaces or `_` between thousands, a
/// percent or a currency sign around it; its `marks` as the column writes them (commas
/// between thousands: `1,204.5`; a decimal comma: `1,5`, `1.204,5`).
pub fn number(t: &str, marks: Marks) -> Option<f64> {
    let t = t.trim().trim_end_matches('%').trim_start_matches(['$', '€', '£']).trim();
    if t.is_empty() {
        return None;
    }
    let blank = |c: &char| matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '_');
    let cleaned: String = match marks {
        Marks::DecimalComma => t
            .chars()
            .filter(|c| *c != '.' && !blank(c))
            .map(|c| if c == ',' { '.' } else { c })
            .collect(),
        Marks::Thousands if grouped_by_commas(t) => {
            t.chars().filter(|c| *c != ',' && !blank(c)).collect()
        }
        Marks::Thousands => return None,
        Marks::Plain => t.chars().filter(|c| !blank(c)).collect(),
    };
    let first = cleaned.chars().next()?;
    if !(first.is_ascii_digit() || matches!(first, '-' | '+' | '.')) {
        return None;
    }
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Whether `t`'s commas all stand between thousands: its whole part one to three digits,
/// then groups of three each after a comma (`1,204`, `-12,500.75`); no comma at all is a
/// plain number too.
fn grouped_by_commas(t: &str) -> bool {
    let t = t.trim_start_matches(['-', '+']);
    let whole = t.split(['.', 'e', 'E']).next().unwrap_or("");
    let mut groups = whole.split(',');
    let first = groups.next().unwrap_or("");
    let digits = |g: &str| g.bytes().all(|b| b.is_ascii_digit());
    (1..=3).contains(&first.len())
        && digits(first)
        && groups.all(|g| g.len() == 3 && digits(g))
        // Commas only in the whole part.
        && !t[whole.len()..].contains(',')
}
