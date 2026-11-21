//! A date range picker any view can open in a popover (the Git log's Date filter; later the
//! grids' date filters): presets on the left (Today, Yesterday, Last 7 / 30 days, This /
//! Last month, This year, and Any time when the caller wants it), on the right the From → To
//! fields (typed in the locale's format, red until they're a day) over two months side by
//! side (one when narrow) — today ringed, the range filled with the accent (its ends solid,
//! between them tinted), the end under the mouse previewed, the days the caller disallows
//! dimmed (the future, for commits) — and a footer saying what to pick, with Cancel and
//! Apply. Keys: ← → ↑ ↓ move the day, Page Up / Down the month, Space / ↵ pick, ↵ applies a
//! whole range, Esc cancels.
//!
//! Dates are calendar days ([`Date`]): the caller says which is today (its local day) and
//! turns the range into moments itself. The month and weekday names, the first day of the
//! week and the date format come from the locales (`time.*`).

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{TextInput, draw_text};

use crate::ViewerKey;
use crate::ds::{self, Ctx, radius};
use crate::tool_panel::KeyReach;

/// A calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// `None` when it isn't a day of the calendar (the 30th of February).
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month))
            .then_some(Self { year, month, day })
    }

    /// Days since 1970-01-01 (negative before).
    pub fn days(self) -> i64 {
        let (y, m) = (i64::from(self.year), i64::from(self.month));
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The day `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }

    /// `n` days later (earlier when negative).
    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.days() + n)
    }

    /// The weekday: 0 Monday … 6 Sunday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (3).
        (self.days() + 3).rem_euclid(7) as u32
    }

    /// The first day of its month.
    pub fn first_of_month(self) -> Self {
        Self { day: 1, ..self }
    }

    /// The same day `n` months later (earlier when negative), cut to the month's end.
    pub fn add_months(self, n: i32) -> Self {
        let index = self.year * 12 + self.month as i32 - 1 + n;
        let (year, month) = (index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
        Self { year, month, day: self.day.min(days_in_month(year, month)) }
    }
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// The days a month's grid shows: six weeks from the week holding its first day (`week
/// start`: 0 Monday, 6 Sunday), the days of the months around it included.
pub fn month_grid(year: i32, month: u32, week_start: u32) -> [Date; 42] {
    let first = Date { year, month, day: 1 };
    let back = (first.weekday() + 7 - week_start) % 7;
    let start = first.add_days(-i64::from(back));
    std::array::from_fn(|i| start.add_days(i as i64))
}

/// What the locale says of dates: its names, its first weekday, its format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateLocale {
    pub months: Vec<String>,
    pub months_short: Vec<String>,
    /// Monday first.
    pub weekdays: Vec<String>,
    /// 0 Monday … 6 Sunday.
    pub week_start: u32,
    /// `dd.mm.yyyy`, `yyyy-mm-dd`, `mm/dd/yyyy`…
    pub format: String,
}

impl DateLocale {
    /// The current language's (`time.months`, `time.weekdays_short`, `time.week_start`,
    /// `time.date_format`).
    pub fn current() -> Self {
        let list = |key: &str| i18n::t(key).split(',').map(|s| s.trim().to_owned()).collect();
        Self {
            months: list("time.months"),
            months_short: list("time.months_short"),
            weekdays: list("time.weekdays_short"),
            week_start: i18n::t("time.week_start").trim().parse().unwrap_or(0),
            format: i18n::t("time.date_format"),
        }
    }

    /// `date` in the locale's format.
    pub fn format(&self, date: Date) -> String {
        self.format
            .replace("yyyy", &format!("{:04}", date.year))
            .replace("mm", &format!("{:02}", date.month))
            .replace("dd", &format!("{:02}", date.day))
    }

    /// A date typed in the locale's format (its separators any of `. / -` and spaces, its
    /// day and month with one digit or two).
    pub fn parse(&self, typed: &str) -> Option<Date> {
        let order: Vec<char> = self
            .format
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter_map(|part| part.chars().next())
            .collect();
        let numbers: Vec<&str> =
            typed.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
        if numbers.len() != 3 || order.len() != 3 {
            return None;
        }
        let (mut year, mut month, mut day) = (None, None, None);
        for (kind, number) in order.iter().zip(&numbers) {
            match kind {
                'y' if number.len() == 4 => year = number.parse().ok(),
                'm' if number.len() <= 2 => month = number.parse().ok(),
                'd' if number.len() <= 2 => day = number.parse().ok(),
                _ => return None,
            }
        }
        Date::new(year?, month?, day?)
    }

    /// A range told shortly: `16 – 31 Oct`, `28 Sep – 3 Oct`, `30 Dec 2025 – 2 Jan 2026`
    /// (the year when it isn't `today`'s).
    pub fn range(&self, (from, to): (Date, Date), today: Date) -> String {
        let month =
            |d: Date| self.months_short.get(d.month as usize - 1).cloned().unwrap_or_default();
        let year =
            |d: Date| if d.year == today.year { String::new() } else { format!(" {}", d.year) };
        if from == to {
            return format!("{} {}{}", from.day, month(from), year(from));
        }
        if (from.year, from.month) == (to.year, to.month) {
            return format!("{} – {} {}{}", from.day, to.day, month(to), year(to));
        }
        if from.year == to.year {
            return format!("{} {} – {} {}{}", from.day, month(from), to.day, month(to), year(to));
        }
        format!("{} {} {} – {} {} {}", from.day, month(from), from.year, to.day, month(to), to.year)
    }
}

/// A preset: its name and its days (`None`: any time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub label: String,
    pub range: Option<(Date, Date)>,
}

/// The standard presets for `today`: Any time first when `any_time`, then Today,
/// Yesterday, the last 7 and 30 days, this month, last month, this year.
pub fn standard_presets(today: Date, any_time: bool) -> Vec<Preset> {
    let p = |key: &str, range| Preset { label: i18n::t(key), range };
    let month = today.first_of_month();
    let last_month = month.add_months(-1);
    let mut out = Vec::new();
    if any_time {
        out.push(p("time.picker_any", None));
    }
    out.extend([
        p("time.picker_today", Some((today, today))),
        p("time.picker_yesterday", Some((today.add_days(-1), today.add_days(-1)))),
        p("time.picker_last_7", Some((today.add_days(-6), today))),
        p("time.picker_last_30", Some((today.add_days(-29), today))),
        p("time.picker_this_month", Some((month, today))),
        p("time.picker_last_month", Some((last_month, month.add_days(-1)))),
        p("time.picker_this_year", Some((Date { year: today.year, month: 1, day: 1 }, today))),
    ]);
    out
}

/// What the picker was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEvent {
    /// Preset `i` chosen (applied at once).
    Preset(usize),
    /// A range chosen and applied.
    Apply(Date, Date),
    Cancel,
}

/// Its parts that take a press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Preset(usize),
    Day(Date),
    Prev,
    Next,
    From,
    To,
    Cancel,
    Apply,
    Inside,
}

/// Which field has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerField {
    From,
    To,
}

/// Sizes, logical pixels.
const PRESETS_W: f32 = 168.0;
const CELL: f32 = 32.0;
const PAD: f32 = 14.0;
const FIELDS_H: f32 = 32.0;
const HEADER_H: f32 = 30.0;
const WEEKDAYS_H: f32 = 22.0;
const FOOTER_H: f32 = 52.0;
const MONTH_GAP: f32 = 20.0;

pub struct DateRangePicker {
    pub locale: DateLocale,
    pub today: Date,
    /// The last day that may be picked (commits: today); `None`: any.
    pub max: Option<Date>,
    pub presets: Vec<Preset>,
    /// The preset in force (checked).
    pub preset: Option<usize>,
    /// The range picked (its first day, and its last once picked).
    pub start: Option<Date>,
    pub end: Option<Date>,
    /// The first month shown.
    pub month: Date,
    /// The day the keys are on.
    pub cursor: Date,
    pub from: TextInput,
    pub to: TextInput,
    pub field: Option<PickerField>,
    hover: Option<Part>,
    hits: Vec<(Rect, Part)>,
    rect: Rect,
    /// Two months shown (else one: narrow).
    pub two: bool,
}

impl std::fmt::Debug for DateRangePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateRangePicker")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish_non_exhaustive()
    }
}

impl DateRangePicker {
    /// For `today` (the caller's local day), with `presets`; `range`: what's chosen now.
    pub fn new(today: Date, presets: Vec<Preset>, range: Option<(Date, Date)>) -> Self {
        let locale = DateLocale::current();
        let mut picker = Self {
            today,
            max: None,
            preset: None,
            start: None,
            end: None,
            month: today.first_of_month().add_months(-1),
            cursor: today,
            from: TextInput::default(),
            to: TextInput::default(),
            field: None,
            hover: None,
            hits: Vec::new(),
            rect: Rect::default(),
            two: true,
            presets,
            locale,
        };
        if let Some((from, to)) = range {
            picker.set_range(from, to);
            picker.month = to.first_of_month().add_months(-1);
            picker.cursor = to;
        }
        picker
    }

    /// The range shown and typed (in order).
    fn set_range(&mut self, a: Date, b: Date) {
        let (from, to) = if a <= b { (a, b) } else { (b, a) };
        self.start = Some(from);
        self.end = Some(to);
        self.from.set_text(&self.locale.format(from));
        self.to.set_text(&self.locale.format(to));
        self.preset = self.presets.iter().position(|p| p.range == Some((from, to)));
    }

    /// The range picked whole.
    pub fn range(&self) -> Option<(Date, Date)> {
        Some((self.start?, self.end?))
    }

    fn allowed(&self, day: Date) -> bool {
        self.max.is_none_or(|m| day <= m)
    }

    /// Day `day` picked: the first one, then the last (one before the first: they swap).
    pub fn pick(&mut self, day: Date) {
        if !self.allowed(day) {
            return;
        }
        self.cursor = day;
        match (self.start, self.end) {
            (Some(start), None) => self.set_range(start, day),
            _ => {
                self.start = Some(day);
                self.end = None;
                self.from.set_text(&self.locale.format(day));
                self.to.set_text("");
                self.preset = None;
            }
        }
    }

    /// The months shown (one or two) keep `day` in view.
    fn show(&mut self, day: Date) {
        let shown = if self.two { 2 } else { 1 };
        let first = self.month;
        let last = first.add_months(shown - 1);
        if day < first {
            self.month = day.first_of_month();
        } else if day.first_of_month() > last {
            self.month = day.first_of_month().add_months(1 - shown);
        }
    }

    /// The fields' text read (typed): valid days become the range's ends.
    fn read_fields(&mut self) {
        let from = self.locale.parse(self.from.text());
        let to = self.locale.parse(self.to.text());
        match (from, to) {
            (Some(a), Some(b)) => {
                let (from_text, to_text) = (self.from.text().to_owned(), self.to.text().to_owned());
                self.set_range(a, b);
                // What's typed stays as typed.
                self.from.set_text(&from_text);
                self.to.set_text(&to_text);
                self.show(b);
            }
            (Some(a), None) => {
                self.start = Some(a);
                self.end = None;
                self.show(a);
            }
            _ => {}
        }
    }

    fn bad(&self, field: PickerField) -> bool {
        let input = if field == PickerField::From { &self.from } else { &self.to };
        let typed = input.text().trim();
        !typed.is_empty() && self.locale.parse(typed).is_none_or(|d| !self.allowed(d))
    }

    /// Apply can be pressed: a whole range, its fields days.
    pub fn can_apply(&self) -> bool {
        self.range().is_some() && !self.bad(PickerField::From) && !self.bad(PickerField::To)
    }

    /// A press at `p`. `None`: outside it (the caller closes it).
    pub fn press(&mut self, p: Point) -> Option<Option<PickerEvent>> {
        if !self.rect.contains(p) {
            return None;
        }
        let part = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        if !matches!(part, Some(Part::From | Part::To)) {
            self.field = None;
        }
        Some(match part {
            Some(Part::Preset(i)) => {
                self.preset = Some(i);
                if let Some((a, b)) = self.presets.get(i).and_then(|p| p.range) {
                    self.set_range(a, b);
                }
                Some(PickerEvent::Preset(i))
            }
            Some(Part::Day(day)) => {
                self.pick(day);
                None
            }
            Some(Part::Prev) => {
                self.month = self.month.add_months(-1);
                None
            }
            Some(Part::Next) => {
                self.month = self.month.add_months(1);
                None
            }
            Some(Part::From) => {
                self.field = Some(PickerField::From);
                self.from.select_all();
                None
            }
            Some(Part::To) => {
                self.field = Some(PickerField::To);
                self.to.select_all();
                None
            }
            Some(Part::Cancel) => Some(PickerEvent::Cancel),
            Some(Part::Apply) => self.apply(),
            Some(Part::Inside) | None => None,
        })
    }

    fn apply(&self) -> Option<PickerEvent> {
        let (a, b) = self.range()?;
        self.can_apply().then_some(PickerEvent::Apply(a, b))
    }

    /// The mouse at `p`: the end previewed. `true` if a redraw is needed.
    pub fn hover(&mut self, p: Point) -> bool {
        let part = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        std::mem::replace(&mut self.hover, part) != part
    }

    /// Typed text, for the field with the keys. `false`: none has them.
    pub fn text_input(&mut self, typed: &str) -> bool {
        let Some(input) = self.field_mut() else { return false };
        input.insert(typed);
        self.read_fields();
        true
    }

    fn field_mut(&mut self) -> Option<&mut TextInput> {
        match self.field? {
            PickerField::From => Some(&mut self.from),
            PickerField::To => Some(&mut self.to),
        }
    }

    /// What's selected in the field with the keys.
    pub fn selected_text(&self) -> Option<String> {
        let input = match self.field? {
            PickerField::From => &self.from,
            PickerField::To => &self.to,
        };
        let s = input.selected_text();
        (!s.is_empty()).then(|| s.to_owned())
    }

    /// An edit command for the field with the keys (⌘A, ⌘X, ⌘Z, ⇧⌘Z).
    pub fn command(&mut self, id: &str) -> bool {
        let Some(input) = self.field_mut() else { return false };
        match id {
            "select_all" => input.select_all(),
            "cut" => input.insert(""),
            "undo" => {
                input.undo();
            }
            "redo" => {
                input.redo();
            }
            _ => return false,
        }
        self.read_fields();
        true
    }

    /// A key: in a field, editing (Tab to the other, ↵ applies); else the days — the arrows
    /// move, Page Up / Down a month, Space / ↵ pick (↵ with a whole range: applied), Esc
    /// cancels.
    pub fn key(&mut self, key: ViewerKey, reach: KeyReach) -> Option<PickerEvent> {
        if key == ViewerKey::Escape {
            return Some(PickerEvent::Cancel);
        }
        if let Some(field) = self.field {
            let input = self.field_mut()?;
            match key {
                ViewerKey::Left => input.move_left(reach.word, reach.extend),
                ViewerKey::Right => input.move_right(reach.word, reach.extend),
                ViewerKey::Home => input.home(reach.extend),
                ViewerKey::End => input.end(reach.extend),
                ViewerKey::Backspace => input.backspace(reach.word),
                ViewerKey::Delete => input.delete(),
                ViewerKey::Tab => {
                    let next = if field == PickerField::From {
                        PickerField::To
                    } else {
                        PickerField::From
                    };
                    self.field = Some(next);
                    if let Some(input) = self.field_mut() {
                        input.select_all();
                    }
                    return None;
                }
                ViewerKey::Enter => return self.apply(),
                _ => return None,
            }
            self.read_fields();
            return None;
        }
        let day = self.cursor;
        let moved = match key {
            ViewerKey::Left => day.add_days(-1),
            ViewerKey::Right => day.add_days(1),
            ViewerKey::Up => day.add_days(-7),
            ViewerKey::Down => day.add_days(7),
            ViewerKey::PageUp => day.add_months(-1),
            ViewerKey::PageDown => day.add_months(1),
            ViewerKey::Home => day.first_of_month(),
            ViewerKey::End => Date { day: days_in_month(day.year, day.month), ..day },
            // A whole range: applied (Space picks anew).
            ViewerKey::Enter if self.range().is_some() => return self.apply(),
            ViewerKey::Enter => {
                self.pick(day);
                return None;
            }
            ViewerKey::Tab => {
                self.field = Some(PickerField::From);
                self.from.select_all();
                return None;
            }
            _ => return None,
        };
        self.cursor = moved;
        self.show(moved);
        None
    }

    /// Space picks the day the keys are on (`false`: a field has the keys, it's typed).
    pub fn space(&mut self) -> bool {
        if self.field.is_some() {
            return false;
        }
        self.pick(self.cursor);
        true
    }

    /// Its size (logical pixels) for two months or one.
    fn size(two: bool) -> (f32, f32) {
        let month = CELL * 7.0;
        let months = if two { month * 2.0 + MONTH_GAP } else { month };
        let w = PRESETS_W + PAD * 3.0 + months;
        let h = PAD * 2.0 + FIELDS_H + 12.0 + HEADER_H + WEEKDAYS_H + CELL * 6.0 + FOOTER_H;
        (w, h)
    }

    /// Where it's drawn (from the last paint): a press outside it closes it.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// It under `anchor` (the pill opening it), within `view`: two months when they fit,
    /// one when narrow; over `anchor` when there's no room below.
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        anchor: Rect,
        view: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let room = view.size.width - px(16.0);
        self.two = px(Self::size(true).0) <= room;
        let (w, h) = Self::size(self.two);
        let (w, h) = (px(w).min(room), px(h));
        let gap = px(ds::POPOVER_GAP);
        let below = anchor.max_y() + px(4.0);
        let y = if below + h <= view.max_y() - gap {
            below
        } else {
            (anchor.min_y() - px(4.0) - h).max(view.min_y() + px(4.0))
        };
        let x = anchor.min_x().min(view.max_x() - w - px(8.0)).max(view.min_x() + px(8.0));
        let rect = Rect::new(x.round(), y.round(), w.round(), h.round());
        self.rect = rect;
        self.hits.clear();
        self.hits.push((rect, Part::Inside));
        let p = ds.palette;
        let layer = scene.push_layer(Some(view));
        ds::surface_with(layer, ds, rect, ds::Elevation::High, radius::LG, p.hover_background);
        let layer = scene.push_layer(Some(rect));
        self.paint_presets(layer, text, icons, ds, rect);
        let left = rect.min_x() + px(PRESETS_W) + px(PAD) * 2.0;
        ds::divider_vertical(
            layer,
            ds,
            (left - px(PAD)).round(),
            rect.min_y() + px(PAD),
            rect.max_y() - px(FOOTER_H),
        );
        let mut y = rect.min_y() + px(PAD);
        self.paint_fields(layer, text, ds, left, y, rect.max_x() - px(PAD));
        y += px(FIELDS_H) + px(12.0);
        let months = if self.two { 2 } else { 1 };
        for i in 0..months {
            let x = left + i as f32 * (px(CELL) * 7.0 + px(MONTH_GAP));
            self.paint_month(
                &mut crate::LayerPainter::new(layer, text, icons, *ds),
                self.month.add_months(i),
                (x, y),
                (i == 0, i == months - 1),
            );
        }
        self.paint_footer(layer, text, ds, rect);
    }

    fn paint_presets(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        rect: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let row_h = px(30.0);
        let x = rect.min_x() + px(PAD) - px(6.0);
        for (i, preset) in self.presets.iter().enumerate() {
            let r = Rect::new(
                x,
                rect.min_y() + px(PAD) + i as f32 * row_h,
                px(PRESETS_W) + px(6.0),
                row_h,
            );
            let row = ds::CheckRowSpec::new(
                &preset.label,
                self.preset == Some(i),
                self.hover == Some(Part::Preset(i)),
            );
            ds::check_row(layer, text, icons, ds, r, row);
            self.hits.push((r, Part::Preset(i)));
        }
    }

    fn paint_fields(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        left: f32,
        y: f32,
        right: f32,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let arrow = text.shape_line("→", &ds.ui_font(0.0));
        let field_w = ((right - left - arrow.width - px(24.0)) / 2.0).max(px(80.0)).min(px(200.0));
        let h = px(FIELDS_H);
        let placeholder = self.locale.format.to_uppercase();
        for (i, field) in [PickerField::From, PickerField::To].into_iter().enumerate() {
            let x = left + i as f32 * (field_w + arrow.width + px(24.0));
            let r = Rect::new(x, y, field_w, h);
            let bad = self.bad(field);
            let mut style = ds::search_input_style(ds, h);
            style.lead = 0.0;
            style.padding = px(10.0);
            style.radius = px(radius::MD);
            style.font = ds.code_font(-0.5);
            if bad {
                style.border = p.danger;
                style.border_focused = p.danger;
            }
            let input = if field == PickerField::From { &self.from } else { &self.to };
            input.paint(layer, text, r, self.field == Some(field), &placeholder, &style);
            self.hits.push((r, if field == PickerField::From { Part::From } else { Part::To }));
            if i == 0 {
                draw_text(
                    layer,
                    &arrow,
                    r.max_x() + px(12.0),
                    ds::optical_baseline(y, h, &ds.ui_font(0.0)),
                    p.text_muted,
                );
            }
        }
    }

    fn paint_month(
        &mut self,
        pen: &mut crate::LayerPainter<'_>,
        month: Date,
        (x, y): (f32, f32),
        (first, last): (bool, bool),
    ) {
        let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let cell = px(CELL);
        let width = cell * 7.0;
        // The header: ‹ Month Year ›.
        let strong = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let name = self.locale.months.get(month.month as usize - 1).cloned().unwrap_or_default();
        let title = text.shape_line(&format!("{name} {}", month.year), &strong);
        draw_text(
            layer,
            &title,
            (x + (width - title.width) / 2.0).round(),
            ds::optical_baseline(y, px(HEADER_H), &strong),
            p.text,
        );
        let s = px(ds::button_size::COMPACT);
        for (show, part, icon, ax) in [
            (first, Part::Prev, "chevron-left", x),
            (last, Part::Next, "chevron-right", x + width - s),
        ] {
            if !show {
                continue;
            }
            let r = Rect::new(ax, y + (px(HEADER_H) - s) / 2.0, s, s);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon(icon)
                .rest_tone(ds::Tone::Neutral)
                .hovered(self.hover == Some(part));
            ds::button(layer, text, icons, ds, r, spec);
            self.hits.push((r, part));
        }
        // The weekdays' initials, from the locale's first one.
        let small = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
        let wy = y + px(HEADER_H);
        for c in 0..7u32 {
            let wd = ((self.locale.week_start + c) % 7) as usize;
            let label = self.locale.weekdays.get(wd).cloned().unwrap_or_default();
            let shaped = text.shape_line(&label, &small);
            let cx = x + c as f32 * cell + (cell - shaped.width) / 2.0;
            draw_text(
                layer,
                &shaped,
                cx.round(),
                ds::optical_baseline(wy, px(WEEKDAYS_H), &small),
                p.text_muted,
            );
        }
        // The days.
        let font = ds.ui_font(-0.5);
        let gy = wy + px(WEEKDAYS_H);
        let preview_end = match (self.start, self.end, self.hover) {
            (Some(_), None, Some(Part::Day(d))) if self.allowed(d) => Some(d),
            _ => None,
        };
        let span = match (self.start, self.end.or(preview_end)) {
            (Some(a), Some(b)) => Some(if a <= b { (a, b) } else { (b, a) }),
            (Some(a), None) => Some((a, a)),
            _ => None,
        };
        for (i, day) in
            month_grid(month.year, month.month, self.locale.week_start).into_iter().enumerate()
        {
            if day.month != month.month {
                continue;
            }
            let (row, col) = (i / 7, i % 7);
            let r = Rect::new(x + col as f32 * cell, gy + row as f32 * cell, cell, cell);
            let inner =
                Rect::new(r.min_x() + px(1.0), r.min_y() + px(1.0), cell - px(2.0), cell - px(2.0));
            let allowed = self.allowed(day);
            let in_span = span.is_some_and(|(a, b)| a <= day && day <= b);
            let is_end = span.is_some_and(|(a, b)| day == a || day == b);
            if in_span && !is_end {
                layer.quad(Quad::filled(
                    Rect::new(r.min_x(), inner.min_y(), cell, inner.size.height),
                    p.accent.with_alpha(0.16),
                ));
            }
            if is_end {
                layer.quad(Quad::filled(inner, p.accent).rounded(px(radius::MD)));
                // A range's ends join the days between: square on their inner side, out to
                // the cell's edge (a single day stays round).
                if let Some((a, b)) = span.filter(|(a, b)| a != b) {
                    let mid = inner.min_x() + inner.size.width / 2.0;
                    let joined = if day == a {
                        Rect::new(mid, inner.min_y(), r.max_x() - mid, inner.size.height)
                    } else {
                        debug_assert_eq!(day, b);
                        Rect::new(r.min_x(), inner.min_y(), mid - r.min_x(), inner.size.height)
                    };
                    layer.quad(Quad::filled(joined, p.accent));
                }
            } else if allowed && self.hover == Some(Part::Day(day)) {
                layer.quad(Quad::filled(inner, p.text.with_alpha(0.08)).rounded(px(radius::MD)));
            }
            if day == self.today && !is_end {
                layer.quad(
                    Quad::filled(inner, Rgba::TRANSPARENT)
                        .rounded(px(radius::MD))
                        .bordered(ds.line().max(px(1.5)), p.accent),
                );
            }
            // The keys' day ringed — not a range's end: solid already, a ring on it would
            // set one end apart from the other.
            if day == self.cursor && self.field.is_none() && !is_end {
                layer.quad(
                    Quad::filled(inner, Rgba::TRANSPARENT)
                        .rounded(px(radius::MD))
                        .bordered(ds.line(), p.text.with_alpha(0.5)),
                );
            }
            let color = if is_end {
                Rgba::WHITE
            } else if !allowed {
                p.text_muted.with_alpha(0.45)
            } else {
                p.text
            };
            let shaped = text.shape_line(&day.day.to_string(), &font);
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (cell - shaped.width) / 2.0).round(),
                ds::optical_baseline(r.min_y(), cell, &font),
                color,
            );
            if allowed {
                self.hits.push((r, Part::Day(day)));
            }
        }
    }

    fn paint_footer(&mut self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>, rect: Rect) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let top = rect.max_y() - px(FOOTER_H);
        ds::divider(layer, ds, rect.min_x(), rect.max_x(), top);
        let mid = top + px(FOOTER_H) / 2.0;
        let hint = match (self.start, self.end) {
            (None, _) => i18n::t("time.picker_pick_first"),
            (Some(_), None) => i18n::t("time.picker_pick_last"),
            (Some(a), Some(b)) => self.locale.range((a, b), self.today),
        };
        let small = ds.ui_font(-1.0);
        let shaped = text.shape_line(&hint, &small);
        draw_text(
            layer,
            &shaped,
            rect.min_x() + px(PAD),
            ds::optical_baseline(top, px(FOOTER_H), &small),
            p.text_muted,
        );
        let (apply, cancel) = (i18n::t("time.picker_apply"), i18n::t("time.picker_cancel"));
        let bw = ds::primary_width(text, ds, &[&apply, &cancel]);
        let bh = px(ds::BUTTON_H);
        let apply_r = Rect::new(rect.max_x() - px(PAD) - bw, (mid - bh / 2.0).round(), bw, bh);
        let cancel_r = Rect::new(apply_r.min_x() - px(8.0) - bw, apply_r.min_y(), bw, bh);
        ds::secondary_button(
            layer,
            text,
            ds,
            cancel_r,
            &cancel,
            true,
            self.hover == Some(Part::Cancel),
        );
        ds::primary_button(
            layer,
            text,
            ds,
            apply_r,
            &apply,
            self.can_apply(),
            self.hover == Some(Part::Apply),
        );
        self.hits.push((cancel_r, Part::Cancel));
        self.hits.push((apply_r, Part::Apply));
    }

    /// A field has the keys (typing goes to it).
    pub fn typing(&self) -> bool {
        self.field.is_some()
    }

    /// The pointer is over a field (an I-beam).
    pub fn over_field(&self, p: Point) -> bool {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(p))
            .is_some_and(|(_, h)| matches!(h, Part::From | Part::To))
    }
}

/// What a press or a key on an open date picker said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeAnswer {
    /// Pressed outside it (its owner closes it).
    Outside,
    /// A preset, a range applied, or cancelled.
    Chose(PickerEvent),
}

/// An open date picker as a form's control: hit anywhere (a press outside it is told, its
/// owner closes it), its fields take the keys and the text; what it said is written to
/// `answer`.
pub struct RangePart<'a> {
    pub picker: &'a mut DateRangePicker,
    pub answer: &'a mut Option<RangeAnswer>,
}

impl std::fmt::Debug for RangePart<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RangePart").finish_non_exhaustive()
    }
}

impl crate::control::Control for RangePart<'_> {
    fn hit(&self, _p: Point) -> bool {
        true
    }

    fn press(&mut self, p: Point, _clicks: u32) -> crate::control::Press {
        *self.answer = match self.picker.press(p) {
            None => Some(RangeAnswer::Outside),
            Some(event) => event.map(RangeAnswer::Chose),
        };
        crate::control::Press::Took
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.picker.hover(p.unwrap_or(crate::control::NOWHERE))
    }

    /// A hand over what takes a press in it, the arrow elsewhere (never what's under it).
    fn cursor(&self, p: Point) -> Option<crate::ViewerCursor> {
        let over = self.picker.hits.iter().any(|(r, _)| r.contains(p));
        Some(if over { crate::ViewerCursor::Pointer } else { crate::ViewerCursor::Arrow })
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if let Some(event) = self.picker.key(key, reach) {
            *self.answer = Some(RangeAnswer::Chose(event));
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.picker.text_input(typed)
    }

    fn selected_text(&self) -> Option<String> {
        self.picker.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        self.picker.command(id)
    }
}

#[cfg(test)]
#[path = "../tests/unit/date_range.rs"]
mod tests;
