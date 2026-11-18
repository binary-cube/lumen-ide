//! How long ago a session was last active, as the list shows it: `now`, `5m`, `18h`, `3d`,
//! `6w`, then the date.

/// `seconds` ago (negative: a clock ahead, read as now), shortly.
pub fn short(seconds: i64) -> String {
    let s = seconds.max(0);
    let n = |key: &str, n: i64| i18n::tr!(key, n = n);
    match s {
        0..60 => i18n::t("agents.age_now"),
        60..3_600 => n("agents.age_minutes", s / 60),
        3_600..86_400 => n("agents.age_hours", s / 3_600),
        86_400..1_209_600 => n("agents.age_days", s / 86_400),
        _ => n("agents.age_weeks", s / 604_800),
    }
}

#[cfg(test)]
#[path = "../tests/unit/age.rs"]
mod tests;
