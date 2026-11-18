//! Times in the user's own time zone (daylight saving included): a moment as shown, its day
//! and time, a day as a heading, today, and when a day starts here.

use chrono::{Datelike, Local, NaiveDate, TimeZone};

/// `2025-11-06 10:30`, here.
pub fn stamp(seconds: i64) -> String {
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .map_or_else(String::new, |d| d.format("%Y-%m-%d %H:%M").to_string())
}

/// A moment's day and time (`14:32`), here.
pub fn day_and_time(seconds: i64) -> Option<(ui_components::Date, String)> {
    let at = Local.timestamp_opt(seconds, 0).single()?;
    let day = ui_components::Date { year: at.year(), month: at.month(), day: at.day() };
    Some((day, at.format("%H:%M").to_string()))
}

/// A day as a heading, by `today`: its name — `Today`, `Yesterday`, else its weekday
/// (`Friday`) — and its date (`13 Nov 2026`), as the language writes them.
pub fn day_heading(day: ui_components::Date, today: ui_components::Date) -> (String, String) {
    let name = if day == today {
        i18n::t("remote.changed_today")
    } else if day == today.add_days(-1) {
        i18n::t("remote.changed_yesterday")
    } else {
        let names = i18n::t("time.weekdays");
        let name = names.split(',').nth(day.weekday() as usize).map(str::trim).unwrap_or("");
        let mut chars = name.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
    };
    let months = i18n::t("time.months_short");
    let month = months.split(',').nth(day.month.saturating_sub(1) as usize).map(str::trim);
    let (d, month, year) = (day.day, month.unwrap_or_default(), day.year);
    (name, i18n::tr!("remote.changed_day", day = d, month = month, year = year))
}

/// Today, here.
pub fn today() -> ui_components::Date {
    let now = Local::now();
    ui_components::Date { year: now.year(), month: now.month(), day: now.day() }
}

/// When calendar day `day` starts here (its local midnight; one skipped by a clock change:
/// the first moment after it), in seconds since the epoch.
pub fn day_start(day: ui_components::Date) -> Option<i64> {
    let midnight = NaiveDate::from_ymd_opt(day.year, day.month, day.day)?.and_hms_opt(0, 0, 0)?;
    let at = Local.from_local_datetime(&midnight).earliest().or_else(|| {
        Local.from_local_datetime(&(midnight + chrono::Duration::hours(1))).earliest()
    })?;
    Some(at.timestamp())
}

#[cfg(test)]
#[path = "../tests/unit/local_time.rs"]
mod tests;
