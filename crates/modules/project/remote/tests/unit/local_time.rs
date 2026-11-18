use super::*;

/// A day starts at local midnight and ends at the next one, whatever the zone; a moment's
/// day and time are its local ones.
#[test]
fn days_and_moments_are_local() {
    let day = ui_components::Date::new(2026, 3, 29).unwrap();
    let start = day_start(day).unwrap();
    let end = day_start(day.add_days(1)).unwrap();
    assert!((23 * 3600..=25 * 3600).contains(&(end - start)), "{}", end - start);
    assert_eq!(&stamp(start)[11..], "00:00");
    assert_eq!(day_and_time(start + 90), Some((day, "00:01".to_owned())));
    assert_eq!(day_and_time(start - 60), Some((day.add_days(-1), "23:59".to_owned())));
    let now = Local::now().timestamp();
    let today_starts = day_start(today()).unwrap();
    assert!(today_starts <= now && now - today_starts < 25 * 3600);
}

/// A day's heading names today and yesterday, another day by its weekday; its date has
/// the day, the month and the year.
#[test]
fn a_day_is_named_by_how_far_it_is() {
    let today = ui_components::Date::new(2026, 11, 15).unwrap();
    let month = i18n::t("time.months_short").split(',').nth(10).unwrap().trim().to_owned();
    let (name, date) = day_heading(today, today);
    assert_eq!(name, i18n::t("remote.changed_today"));
    assert!(date.contains(&month) && date.contains("15") && date.contains("2026"), "{date}");
    assert_eq!(day_heading(today.add_days(-1), today).0, i18n::t("remote.changed_yesterday"));
    // 13 November 2026 is a Friday: its name, capitalized.
    let friday = i18n::t("time.weekdays").split(',').nth(4).unwrap().trim().to_owned();
    let (name, date) = day_heading(today.add_days(-2), today);
    assert_eq!(name.to_lowercase(), friday.to_lowercase());
    assert!(name.chars().next().unwrap().is_uppercase(), "{name}");
    assert!(date.contains("13"), "{date}");
}
