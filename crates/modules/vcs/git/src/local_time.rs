//! Dates in the user's own time zone (daylight saving included): a commit's day as shown,
//! today, and when a day picked for the log's Date filter starts (its local midnight).

use chrono::{Datelike, Local, NaiveDate, TimeZone};

/// The local day (year, month, day) of `seconds` since the epoch.
pub fn date(seconds: i64) -> (i64, u32, u32) {
    match Local.timestamp_opt(seconds, 0).earliest() {
        Some(at) => (i64::from(at.year()), at.month(), at.day()),
        None => version_control::date(seconds),
    }
}

/// When `day` starts here (its local midnight; a midnight skipped by a clock change: the
/// first moment after it), in seconds since the epoch.
fn start_of(day: NaiveDate) -> Option<i64> {
    let midnight = day.and_hms_opt(0, 0, 0)?;
    let at = Local.from_local_datetime(&midnight).earliest().or_else(|| {
        Local.from_local_datetime(&(midnight + chrono::Duration::hours(1))).earliest()
    })?;
    Some(at.timestamp())
}

/// Today, here.
pub fn today() -> ui_components::Date {
    let now = Local::now();
    ui_components::Date { year: now.year(), month: now.month(), day: now.day() }
}

/// When calendar day `day` starts here (its local midnight).
pub fn date_start(day: ui_components::Date) -> Option<i64> {
    start_of(NaiveDate::from_ymd_opt(day.year, day.month, day.day)?)
}

#[cfg(test)]
#[path = "../tests/unit/local_time.rs"]
mod tests;
