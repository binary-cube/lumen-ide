//! A profile's figures as its view writes them, in the current language: times (`0.042 ms`,
//! `845.2 ms`, `2.48 s`), shares (`48 %`), rows grouped by thousands, bytes; and exactly, for
//! the tooltips (`412.345 ms`).

use database::profile::rules::Severity;
use database::profile::{Quantity, Unit};
use ui_components::Level;

use crate::profile::analysis;

/// A time, rounded as the view shows it.
pub fn ms(value: f64) -> String {
    analysis::quantity(Quantity::new(value, Unit::Millis))
}

/// A time with every decimal the engine gave (at most three).
pub fn ms_exact(value: f64) -> String {
    decimal(format!("{} ms", database::profile::export::number(value)))
}

/// A share (0..1) as a percentage: `48 %`, `0.4 %`.
pub fn share(fraction: f64) -> String {
    analysis::quantity(Quantity::new(fraction * 100.0, Unit::Percent))
}

/// A share with one decimal (the tooltips): `48.2 %`.
pub fn share_exact(fraction: f64) -> String {
    decimal(format!("{:.1}%", fraction * 100.0))
}

/// Rows, a count: grouped by thousands.
pub fn count(value: f64) -> String {
    analysis::grouped(value.round() as i64)
}

pub fn bytes(value: u64) -> String {
    i18n::bytes(value)
}

/// A number as written (a cost, a width): grouped, its decimals kept (at most two).
pub fn number(value: f64) -> String {
    if value.fract().abs() < 1e-9 || value.abs() >= 1000.0 {
        count(value)
    } else {
        decimal(format!("{value:.2}"))
    }
}

/// Real rows over estimated ones as a factor (`×120 ↑`, `×0.01 ↓`).
pub fn factor(ratio: f64) -> String {
    analysis::quantity(Quantity::new(ratio, Unit::Ratio))
}

/// The decimal point in the current language.
fn decimal(text: String) -> String {
    if i18n::locale() == "ro" { text.replace('.', ",") } else { text }
}

/// The domain's severity as the design system's.
pub fn level(severity: Severity) -> Level {
    match severity {
        Severity::Info => Level::Info,
        Severity::Warning => Level::Warning,
        Severity::Critical => Level::Critical,
    }
}
