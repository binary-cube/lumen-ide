use super::*;

fn d(year: i32, month: u32, day: u32) -> Date {
    Date::new(year, month, day).unwrap()
}

fn locale(format: &str, week_start: u32) -> DateLocale {
    DateLocale {
        months: (1..=12).map(|m| format!("M{m}")).collect(),
        months_short: [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .map(str::to_owned)
        .to_vec(),
        weekdays: ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(str::to_owned).to_vec(),
        week_start,
        format: format.into(),
    }
}

#[test]
fn days_and_months() {
    assert!(is_leap(2024) && !is_leap(2025) && !is_leap(1900) && is_leap(2000));
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2025, 2), 28);
    assert!(Date::new(2025, 2, 29).is_none());
    for days in [-1000, 0, 59, 10_957, 20_454] {
        assert_eq!(Date::from_days(days).days(), days);
    }
    assert_eq!(d(1970, 1, 1).weekday(), 3, "a Thursday");
    assert_eq!(d(2026, 11, 4).weekday(), 2, "a Wednesday");
    assert_eq!(d(2024, 3, 31).add_months(-1), d(2024, 2, 29), "cut to the month's end");
    assert_eq!(d(2025, 1, 15).add_months(-1), d(2024, 12, 15));
}

/// Six weeks from the week holding the 1st, starting on the locale's first weekday.
#[test]
fn a_month_s_grid_starts_on_the_week_s_first_day() {
    // February 2024 begins on a Thursday.
    let monday = month_grid(2024, 2, 0);
    assert_eq!(monday[0], d(2024, 1, 29));
    assert_eq!(monday[3], d(2024, 2, 1));
    assert_eq!(monday[41], d(2024, 3, 10));
    assert!(monday.contains(&d(2024, 2, 29)));
    let sunday = month_grid(2024, 2, 6);
    assert_eq!(sunday[0], d(2024, 1, 28));
    assert_eq!(sunday[4], d(2024, 2, 1));
    // A month starting on the week's first day: no day of the month before.
    assert_eq!(month_grid(2024, 4, 0)[0], d(2024, 4, 1));
}

#[test]
fn typed_dates_follow_the_locale() {
    let ro = locale("dd.mm.yyyy", 0);
    assert_eq!(ro.parse("16.10.2026"), Some(d(2026, 10, 16)));
    assert_eq!(ro.parse(" 1.2.2024 "), Some(d(2024, 2, 1)));
    assert_eq!(ro.parse("29.02.2025"), None);
    assert_eq!(ro.parse("2026-10-16"), None);
    assert_eq!(ro.format(d(2026, 1, 3)), "03.01.2026");
    let en = locale("yyyy-mm-dd", 6);
    assert_eq!(en.parse("2026-10-16"), Some(d(2026, 10, 16)));
    assert_eq!(en.parse("16.10.2026"), None);
    let us = locale("mm/dd/yyyy", 6);
    assert_eq!(us.parse("10/16/2026"), Some(d(2026, 10, 16)));
    let today = d(2026, 11, 4);
    assert_eq!(ro.range((d(2026, 10, 16), d(2026, 10, 31)), today), "16 – 31 Oct");
    assert_eq!(ro.range((d(2026, 9, 28), d(2026, 10, 3)), today), "28 Sep – 3 Oct");
    assert_eq!(ro.range((d(2025, 12, 30), d(2026, 1, 2)), today), "30 Dec 2025 – 2 Jan 2026");
}

#[test]
fn the_presets_are_days_around_today() {
    let today = d(2026, 3, 1);
    let presets = standard_presets(today, true);
    let range = |i: usize| presets[i].range;
    assert_eq!(range(0), None, "any time");
    assert_eq!(range(1), Some((today, today)));
    assert_eq!(range(2), Some((d(2026, 2, 28), d(2026, 2, 28))));
    assert_eq!(range(3), Some((d(2026, 2, 23), today)));
    assert_eq!(range(4), Some((d(2026, 1, 31), today)));
    assert_eq!(range(5), Some((d(2026, 3, 1), today)));
    assert_eq!(range(6), Some((d(2026, 2, 1), d(2026, 2, 28))));
    assert_eq!(range(7), Some((d(2026, 1, 1), today)));
}

fn picker() -> DateRangePicker {
    let today = d(2026, 11, 4);
    let mut p = DateRangePicker::new(today, standard_presets(today, true), None);
    p.locale = locale("dd.mm.yyyy", 0);
    p.max = Some(today);
    p
}

/// Two days picked make the range, in order whichever is first; not past the last day
/// allowed; a preset sets it; Apply only once it's whole.
#[test]
fn picking_a_range() {
    let mut p = picker();
    assert!(!p.can_apply());
    p.pick(d(2026, 10, 20));
    assert_eq!((p.start, p.end), (Some(d(2026, 10, 20)), None));
    p.pick(d(2026, 10, 12));
    assert_eq!(p.range(), Some((d(2026, 10, 12), d(2026, 10, 20))), "swapped");
    assert_eq!(p.from.text(), "12.10.2026");
    assert!(p.can_apply());
    p.pick(d(2026, 11, 9));
    assert_eq!(p.range(), Some((d(2026, 10, 12), d(2026, 10, 20))), "the future: not picked");
    p.pick(d(2026, 11, 1));
    assert_eq!(p.range(), None, "a new start");
    p.set_range(d(2026, 10, 29), d(2026, 11, 4));
    assert_eq!(p.preset, Some(3), "it's Last 7 days");
}

/// Typed: the fields' days become the range; one that isn't a day is red, Apply off.
#[test]
fn typing_the_days() {
    let mut p = picker();
    p.field = Some(PickerField::From);
    p.text_input("01.10.2026");
    p.field = Some(PickerField::To);
    p.text_input("15.10.2026");
    assert_eq!(p.range(), Some((d(2026, 10, 1), d(2026, 10, 15))));
    assert!(p.can_apply());
    assert!(p.command("select_all"));
    assert_eq!(p.selected_text().as_deref(), Some("15.10.2026"));
    p.text_input("31.02.2026");
    assert!(p.bad(PickerField::To) && !p.can_apply());
}

/// Keys: the arrows move the day, Page Up / Down the month, ↵ picks (and applies a whole
/// range), Esc cancels.
#[test]
fn the_keys_walk_the_days() {
    let mut p = picker();
    let plain = KeyReach::default();
    assert_eq!(p.cursor, d(2026, 11, 4));
    p.key(ViewerKey::Left, plain);
    p.key(ViewerKey::Up, plain);
    assert_eq!(p.cursor, d(2026, 10, 27));
    assert_eq!(p.key(ViewerKey::Enter, plain), None);
    p.key(ViewerKey::PageUp, plain);
    assert_eq!(p.cursor, d(2026, 9, 27));
    assert_eq!(p.month, d(2026, 9, 1), "the month comes into view");
    assert!(p.space());
    assert_eq!(p.range(), Some((d(2026, 9, 27), d(2026, 10, 27))));
    assert_eq!(
        p.key(ViewerKey::Enter, plain),
        Some(PickerEvent::Apply(d(2026, 9, 27), d(2026, 10, 27)))
    );
    assert_eq!(p.key(ViewerKey::Escape, plain), Some(PickerEvent::Cancel));
}

/// A range's ends join the days between: each painted round, then squared on its inner
/// side (the start's right half, the end's left half); a single day stays round.
#[test]
fn a_range_s_ends_are_squared_toward_the_days_between() {
    let palette = crate::palette::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut squares = |p: &mut DateRangePicker| {
        let mut scene = render_api::Scene::default();
        let view = Rect::new(0.0, 0.0, 1200.0, 800.0);
        p.paint(&mut scene, &mut text, &icons, &ds, Rect::new(100.0, 40.0, 80.0, 26.0), view);
        let accent = palette.accent;
        let quads: Vec<_> = scene.layers().iter().flat_map(|l| l.quads.iter()).collect();
        let round = quads
            .iter()
            .filter(|q| q.color == accent && q.corner_radius > 0.0 && q.border_width == 0.0)
            .count();
        let square: Vec<Rect> = quads
            .iter()
            .filter(|q| q.color == accent && q.corner_radius == 0.0)
            .map(|q| q.bounds)
            .collect();
        (round, square)
    };
    let mut p = picker();
    p.set_range(d(2026, 11, 2), d(2026, 11, 4));
    let (round, square) = squares(&mut p);
    assert!(round >= 2, "both ends round first");
    assert_eq!(square.len(), 2, "one square half each");
    assert!(square[0].max_x() <= square[1].min_x(), "the start's half before the end's");
    p.set_range(d(2026, 11, 3), d(2026, 11, 3));
    assert!(squares(&mut p).1.is_empty(), "a single day stays round");
}
