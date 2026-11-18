//! `gantt`: a title, `dateFormat` (`YYYY-MM-DD` and the like), `axisFormat` (`%Y-%m-%d`),
//! `excludes weekends`, sections and tasks — `done` / `active` / `crit` / `milestone`, an
//! id, a start (a date, `after a b`, or where the one before ended) and an end (a date,
//! a duration `3d` / `12h` / `2w`, or `until id`). Times are seconds since 1970, UTC.

use super::{Error, Line, Meta};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Task {
    pub name: String,
    pub id: Option<String>,
    pub section: Option<usize>,
    pub start: f64,
    pub end: f64,
    pub done: bool,
    pub active: bool,
    pub crit: bool,
    pub milestone: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gantt {
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    /// How the axis writes its dates (strftime's `%Y`, `%m`, `%d`, `%b`, `%H`, `%M`).
    pub axis_format: Option<String>,
}

pub const DAY: f64 = 86_400.0;

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Gantt, Error> {
    let mut g = Gantt::default();
    let mut format = "YYYY-MM-DD".to_owned();
    let mut weekends = false;
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "title" => {
                meta.title = Some(rest.to_owned());
                continue;
            }
            "dateFormat" => {
                format = rest.to_owned();
                continue;
            }
            "axisFormat" => {
                g.axis_format = Some(rest.to_owned());
                continue;
            }
            "excludes" => {
                weekends = rest.split(',').any(|w| w.trim() == "weekends");
                continue;
            }
            "section" => {
                g.sections.push(rest.to_owned());
                continue;
            }
            "todayMarker" | "tickInterval" | "weekday" | "includes" | "inclusiveEndDates"
            | "topAxis" | "displayMode" => {
                continue;
            }
            _ => {}
        }
        let (name, data) = t.split_once(':').ok_or_else(|| Error::unexpected(line))?;
        let task = task(&g, name.trim(), data, &format, weekends)
            .ok_or_else(|| Error::unexpected(line))?;
        g.tasks.push(task);
    }
    Ok(g)
}

fn task(g: &Gantt, name: &str, data: &str, format: &str, weekends: bool) -> Option<Task> {
    let mut parts: Vec<&str> = data.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut t =
        Task { name: name.to_owned(), section: g.sections.len().checked_sub(1), ..Task::default() };
    while let Some(first) = parts.first() {
        match *first {
            "done" => t.done = true,
            "active" => t.active = true,
            "crit" => t.crit = true,
            "milestone" => t.milestone = true,
            _ => break,
        }
        parts.remove(0);
    }
    let previous_end = g.tasks.last().map(|t| t.end);
    let is_start = |s: &str| s.starts_with("after ") || date(s, format).is_some();
    let (start, end) = match parts.as_slice() {
        [end] => (previous_end?, *end),
        [a, end] if is_start(a) => (start_of(g, a, format)?, *end),
        [id, end] => {
            t.id = Some((*id).to_owned());
            (previous_end?, *end)
        }
        [id, start, end] => {
            t.id = Some((*id).to_owned());
            (start_of(g, start, format)?, *end)
        }
        _ => return None,
    };
    t.start = start;
    t.end = end_of(g, end, start, format, weekends)?;
    if t.end < t.start {
        return None;
    }
    Some(t)
}

/// `after a b` (the latest of their ends) or a date.
fn start_of(g: &Gantt, s: &str, format: &str) -> Option<f64> {
    if let Some(ids) = s.strip_prefix("after ") {
        return ids
            .split_whitespace()
            .filter_map(|id| g.tasks.iter().find(|t| t.id.as_deref() == Some(id)).map(|t| t.end))
            .reduce(f64::max);
    }
    date(s, format)
}

/// `until id` (its start), a date, or a duration from `start`.
fn end_of(g: &Gantt, s: &str, start: f64, format: &str, weekends: bool) -> Option<f64> {
    if let Some(ids) = s.strip_prefix("until ") {
        return ids
            .split_whitespace()
            .filter_map(|id| g.tasks.iter().find(|t| t.id.as_deref() == Some(id)).map(|t| t.start))
            .reduce(f64::min);
    }
    if let Some(d) = date(s, format) {
        return Some(d);
    }
    let (n, unit) = duration(s)?;
    if unit == 'd' && weekends {
        return Some(add_workdays(start, n));
    }
    let seconds = match unit {
        'S' => n / 1000.0,
        's' => n,
        'm' => n * 60.0,
        'h' => n * 3600.0,
        'd' => n * DAY,
        'w' => n * 7.0 * DAY,
        'M' => n * 30.0 * DAY,
        'y' => n * 365.0 * DAY,
        _ => return None,
    };
    Some(start + seconds)
}

/// `3d`, `1.5h`, `250ms` (`ms` as `S`).
fn duration(s: &str) -> Option<(f64, char)> {
    let s = s.trim();
    let (number, unit) = if let Some(n) = s.strip_suffix("ms") {
        (n, 'S')
    } else {
        let unit = s.chars().last()?;
        (&s[..s.len() - unit.len_utf8()], unit)
    };
    let n: f64 = number.trim().parse().ok()?;
    matches!(unit, 'S' | 's' | 'm' | 'h' | 'd' | 'w' | 'M' | 'y').then_some((n, unit))
}

/// `start` moved by `days` days not counting Saturdays and Sundays.
fn add_workdays(start: f64, days: f64) -> f64 {
    let mut t = start;
    let mut left = days;
    while left > 0.0 {
        t += DAY;
        let weekday = ((t / DAY).floor() as i64 + 4).rem_euclid(7); // 0: Sunday
        if weekday != 0 && weekday != 6 {
            left -= 1.0;
        }
    }
    t
}

/// A date as `format` writes it (`YYYY-MM-DD HH:mm:ss`, `x` for milliseconds).
pub fn date(s: &str, format: &str) -> Option<f64> {
    let s = s.trim();
    if format == "x" {
        return s.parse::<f64>().ok().map(|ms| ms / 1000.0);
    }
    if format == "X" {
        return s.parse::<f64>().ok();
    }
    let (mut y, mut mo, mut d, mut h, mut mi, mut se) = (1970i64, 1i64, 1i64, 0i64, 0i64, 0i64);
    let mut fmt = format;
    let mut text = s;
    let take = |text: &mut &str, max: usize| -> Option<i64> {
        let len = text.chars().take(max).take_while(char::is_ascii_digit).count();
        if len == 0 {
            return None;
        }
        let v = text[..len].parse().ok()?;
        *text = &text[len..];
        Some(v)
    };
    while !fmt.is_empty() {
        let tokens: [(&str, usize); 10] = [
            ("YYYY", 4),
            ("YY", 2),
            ("MM", 2),
            ("M", 2),
            ("DD", 2),
            ("D", 2),
            ("HH", 2),
            ("H", 2),
            ("mm", 2),
            ("ss", 2),
        ];
        if let Some((tok, max)) = tokens.iter().find(|(t, _)| fmt.starts_with(t)) {
            let v = take(&mut text, *max)?;
            match *tok {
                "YYYY" => y = v,
                "YY" => y = 2000 + v,
                "MM" | "M" => mo = v,
                "DD" | "D" => d = v,
                "HH" | "H" => h = v,
                "mm" => mi = v,
                _ => se = v,
            }
            fmt = &fmt[tok.len()..];
        } else {
            let c = fmt.chars().next()?;
            if !text.starts_with(c) {
                return None;
            }
            fmt = &fmt[c.len_utf8()..];
            text = &text[c.len_utf8()..];
        }
    }
    if !text.is_empty() || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se) as f64)
}

/// Days since 1970-01-01 of a date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day since 1970 (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `t` written as `format` (strftime's `%Y %y %m %d %e %b %B %a %H %M %S %j`).
pub fn format_time(t: f64, format: &str) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const LONG: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let days = (t / DAY).floor() as i64;
    let secs = (t - days as f64 * DAY) as i64;
    let (y, m, d) = civil_from_days(days);
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&y.to_string()),
            Some('y') => out.push_str(&format!("{:02}", y % 100)),
            Some('m') => out.push_str(&format!("{m:02}")),
            Some('d') => out.push_str(&format!("{d:02}")),
            Some('e') => out.push_str(&d.to_string()),
            Some('b') => out.push_str(MONTHS[(m - 1) as usize]),
            Some('B') => out.push_str(LONG[(m - 1) as usize]),
            Some('a') => out.push_str(DAYS[(days + 4).rem_euclid(7) as usize]),
            Some('H') => out.push_str(&format!("{:02}", secs / 3600)),
            Some('M') => out.push_str(&format!("{:02}", secs / 60 % 60)),
            Some('S') => out.push_str(&format!("{:02}", secs % 60)),
            Some('j') => out.push_str(&format!("{:03}", days - days_from_civil(y, 1, 1) + 1)),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/gantt.rs"]
mod tests;
