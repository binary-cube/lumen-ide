//! The log's filters, in its bar: "Text or hash" (`.*`: a regular
//! expression, `Cc`: match case), Branch ▾ (the local and remote branches, the tags),
//! User ▾ (the authors, "me" first), Date ▾ (the last 24 hours, 7 or 30 days, an interval),
//! Paths ▾ (the folders and files git tracks) and the order (by date, topologically).
//! They combine; each one set shows as an active pill with its ×. Changing one reads the log
//! again (in the background, page by page, as always); the text applies once typing pauses.
//!
//! The lists (the authors, the files) are read in the background the first time they're
//! opened, and their rows worked out only when what they show changes (opened, typed in,
//! chosen): never per frame.

use std::time::{Duration, Instant};

use kernel::FuzzyStyle;
use ui_components::Date;
use ui_components::search_field::SearchField;
use version_control::{LogFilter, LogOrder, TextFilter, TextMatcher};

use super::*;

/// How long typing pauses before the text applies.
pub(super) const TYPING_PAUSE: Duration = Duration::from_millis(300);
/// How many commits back the authors are looked for.
const AUTHORS_SCAN: usize = 20_000;
/// The rows a list holds at most (more are found by typing).
pub(super) const LIST_ROWS: usize = 300;

/// Paths and names: words start after `/ . _ -` and spaces (as Go to File).
const NAMES: FuzzyStyle = FuzzyStyle {
    separators: &['/', '.', '_', '-', ' '],
    consecutive_bonus: 15,
    boundary_bonus: 10,
    gap_penalty: true,
    prefer_boundaries: true,
};

/// A filter's list (its pill's popover).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum FilterKind {
    Branch,
    User,
    Date,
    Paths,
    Order,
}

impl FilterKind {
    /// The bar's pills, in order.
    pub(super) const ALL: [Self; 5] =
        [Self::Branch, Self::User, Self::Date, Self::Paths, Self::Order];

    /// Its name in the view's commands (`filter:branch`).
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::User => "user",
            Self::Date => "date",
            Self::Paths => "paths",
            Self::Order => "order",
        }
    }

    /// Its list has a search field (and a long list).
    pub(super) fn searched(self) -> bool {
        matches!(self, Self::Branch | Self::User | Self::Paths)
    }
}

/// A field of the bar's that takes the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    /// "Text or hash".
    Text,
    /// The open list's search.
    Query,
    /// The branches panel's filter.
    Branches,
}

/// What a row of a list sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Choice {
    /// A section's title (nothing to choose).
    Header,
    Reference(String),
    Author(String),
    Path(String),
    Order(LogOrder),
}

/// A row of a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub label: String,
    pub detail: String,
    pub icon: &'static str,
    pub choice: Choice,
    pub checked: bool,
    /// The bytes of the label the search matched.
    pub lit: Vec<usize>,
}

/// The days the Date filter keeps (its pill's label): a preset's, or a range picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DateChoice {
    Preset(String, (Date, Date)),
    Range(Date, Date),
}

impl DateChoice {
    fn range(&self) -> (Date, Date) {
        match self {
            Self::Preset(_, range) => *range,
            Self::Range(a, b) => (*a, *b),
        }
    }
}

/// An open list: under its pill.
#[derive(Debug, Default)]
pub(super) struct Popup {
    pub kind: Option<FilterKind>,
    pub query: SearchField,
    pub rows: Vec<Row>,
    /// More rows matched than are listed.
    pub more: usize,
    pub scroll: f32,
    /// The shared scrollbar on its rows' right while they scroll.
    pub bar: ui_components::EdgeScrollbar,
    /// The row the keys move (`↑ ↓`, `↵`).
    pub active: Option<usize>,
    /// Where it was painted (a click outside closes it), and its rows.
    pub rect: Rect,
    pub list: Rect,
}

/// The lists' data, read in the background the first time one is opened.
#[derive(Debug, Default, Clone)]
pub(super) struct Lists {
    pub authors: Vec<String>,
    pub me: Option<String>,
    pub files: Vec<String>,
    /// The folders the files are in (each once, sorted): made with the files, in the
    /// background, not again at every key typed in the Paths list.
    pub folders: Vec<String>,
}

impl Lists {
    /// The folders of `files`: each prefix ending before a `/`, once, sorted.
    pub(super) fn folders_of(files: &[String]) -> Vec<String> {
        let mut folders: Vec<&str> =
            files.iter().flat_map(|f| f.match_indices('/').map(move |(i, _)| &f[..i])).collect();
        folders.sort_unstable();
        folders.dedup();
        folders.into_iter().map(str::to_owned).collect()
    }
}

/// The bar's state (what's applied is the view's [`LogFilter`]).
#[derive(Default)]
pub(super) struct FilterBar {
    pub text: SearchField,
    pub regex: bool,
    pub match_case: bool,
    /// The regular expression typed isn't one.
    pub invalid: bool,
    /// Typed since it was applied (applied once the typing pauses).
    pub typed_at: Option<Instant>,
    pub date: Option<DateChoice>,
    pub popup: Option<Popup>,
    /// The Date filter's picker, when open.
    pub picker: Option<ui_components::DateRangePicker>,
    pub focus: Option<Field>,
    pub lists: Option<Arc<Lists>>,
    pub lists_loading: Option<Receiver<Lists>>,
    /// The Paths list matched in the background (thousands of paths): its generation (a
    /// result for an older search is dropped) and where it comes.
    pub paths_generation: u64,
    pub paths_matching: Option<Receiver<(u64, Vec<Row>, usize)>>,
    /// The branches panel's filter.
    pub branch_query: SearchField,
    /// The pills (from the last paint): where their lists open, their ×.
    pub pills: Vec<(FilterKind, Rect)>,
}

impl LogView {
    /// The log as `filter` has it (read again if it changed).
    pub(super) fn apply_filter(&mut self, filter: LogFilter) {
        if self.filter != filter {
            self.filter = filter;
            self.reload();
        }
    }

    /// The text typed, applied (a regular expression that isn't one leaves the last one).
    pub(super) fn apply_text(&mut self) {
        self.bar.typed_at = None;
        let typed = self.bar.text.text().trim().to_owned();
        let text = (!typed.is_empty()).then_some(TextFilter {
            text: typed,
            regex: self.bar.regex,
            match_case: self.bar.match_case,
        });
        if let Some(text) = &text
            && TextMatcher::new(text).is_err()
        {
            self.bar.invalid = true;
            return;
        }
        self.bar.invalid = false;
        let filter = LogFilter { text, ..self.filter.clone() };
        self.apply_filter(filter);
    }

    /// Typing in the text field: applied after a pause.
    pub(super) fn text_typed(&mut self) {
        self.bar.typed_at = Some(Instant::now());
    }

    /// The typing paused long enough: the text applies. `true` if it did.
    pub(super) fn poll_filters(&mut self) -> bool {
        let mut changed = self.poll_paths();
        if self.bar.typed_at.is_some_and(|t| t.elapsed() >= TYPING_PAUSE) {
            self.apply_text();
            changed = true;
        }
        if let Some(rx) = &self.bar.lists_loading {
            match rx.try_recv() {
                Ok(lists) => {
                    self.bar.lists_loading = None;
                    self.bar.lists = Some(Arc::new(lists));
                    self.refresh_rows();
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.bar.lists_loading = None,
            }
        }
        changed
    }

    /// Opens filter `kind`'s list (closes it if it's the one open); its search field gets
    /// the keys.
    pub(super) fn toggle_popup(&mut self, kind: FilterKind) {
        if self.bar.popup.as_ref().is_some_and(|p| p.kind == Some(kind))
            || (kind == FilterKind::Date && self.bar.picker.is_some())
        {
            self.close_popup();
            return;
        }
        self.close_popup();
        // The Date filter: its picker (the presets, two months; commits aren't in the
        // future).
        if kind == FilterKind::Date {
            let today = crate::local_time::today();
            let presets = ui_components::date_range::standard_presets(today, true);
            let range = self.bar.date.as_ref().map(DateChoice::range);
            let mut picker = ui_components::DateRangePicker::new(today, presets, range);
            picker.max = Some(today);
            if self.bar.date.is_none() {
                picker.preset = Some(0);
            }
            self.bar.picker = Some(picker);
            return;
        }
        let popup = Popup { kind: Some(kind), ..Popup::default() };
        self.bar.popup = Some(popup);
        self.bar.focus = kind.searched().then_some(Field::Query);
        if matches!(kind, FilterKind::User | FilterKind::Paths) {
            self.read_lists();
        }
        self.refresh_rows();
    }

    pub(super) fn close_popup(&mut self) {
        self.bar.popup = None;
        self.bar.picker = None;
        if matches!(self.bar.focus, Some(Field::Query)) {
            self.bar.focus = None;
        }
    }

    /// The authors, who commits here and the files, read in the background (once).
    fn read_lists(&mut self) {
        if self.bar.lists.is_some() || self.bar.lists_loading.is_some() {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-log", move || {
            let files = repo.tracked_files();
            let lists = Lists {
                authors: repo.authors(AUTHORS_SCAN),
                me: repo.user_name(),
                folders: Lists::folders_of(&files),
                files,
            };
            if tx.send(lists).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.bar.lists_loading = Some(rx);
    }

    /// The open list's rows again (opened, typed in, chosen, its data read).
    pub(super) fn refresh_rows(&mut self) {
        let Some(kind) = self.bar.popup.as_ref().and_then(|p| p.kind) else { return };
        let query: Vec<char> = self
            .bar
            .popup
            .as_ref()
            .map(|p| p.query.text().trim().to_lowercase().chars().collect())
            .unwrap_or_default();
        let (rows, more) = match kind {
            FilterKind::Branch => (self.branch_choices(&query), 0),
            FilterKind::User => (self.author_choices(&query), 0),
            FilterKind::Paths => {
                self.match_paths(query);
                return;
            }
            FilterKind::Date => (Vec::new(), 0),
            FilterKind::Order => (self.order_choices(), 0),
        };
        if let Some(popup) = &mut self.bar.popup {
            popup.active = popup.active.filter(|&a| a < rows.len());
            popup.rows = rows;
            popup.more = more;
        }
    }

    fn branch_choices(&self, query: &[char]) -> Vec<Row> {
        let refs = self.refs.as_deref().unwrap_or_default();
        let checked = |name: &str| self.filter.references.iter().any(|r| r == name);
        let mut rows = Vec::new();
        let head = Row {
            label: "HEAD".into(),
            detail: self.sync.as_ref().map(|s| s.branch.clone()).unwrap_or_default(),
            icon: "crosshair",
            choice: Choice::Reference("HEAD".into()),
            checked: checked("HEAD"),
            lit: Vec::new(),
        };
        if query.is_empty() {
            rows.push(head);
        } else if let Some((_, lit)) = kernel::fuzzy_match("HEAD", query, &NAMES) {
            rows.push(Row { lit, ..head });
        }
        let sections = [
            (RefKind::Branch, "git.branches_local", "git-branch"),
            (RefKind::Remote, "git.branches_remote", "git-branch"),
            (RefKind::Tag, "git.branches_tags", "tag"),
        ];
        for (kind, title, icon) in sections {
            let mut found: Vec<(i32, Row)> = refs
                .iter()
                .filter(|r| r.kind == kind)
                .filter_map(|r| {
                    let (score, lit) = if query.is_empty() {
                        (0, Vec::new())
                    } else {
                        kernel::fuzzy_match(&r.name, query, &NAMES)?
                    };
                    let row = Row {
                        label: r.name.clone(),
                        detail: String::new(),
                        icon,
                        choice: Choice::Reference(r.name.clone()),
                        checked: checked(&r.name),
                        lit,
                    };
                    Some((score, row))
                })
                .collect();
            if found.is_empty() {
                continue;
            }
            // The best matches first; unsearched, by name.
            found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
            rows.push(header(&i18n::t(title)));
            rows.extend(found.into_iter().map(|(_, r)| r).take(LIST_ROWS));
        }
        rows
    }

    fn author_choices(&self, query: &[char]) -> Vec<Row> {
        let Some(lists) = &self.bar.lists else { return Vec::new() };
        let checked = |name: &str| self.filter.authors.iter().any(|a| a == name);
        let mut rows = Vec::new();
        let matched = |name: &str, label: String, icon| {
            let (score, lit) = if query.is_empty() {
                (0, Vec::new())
            } else {
                kernel::fuzzy_match(&label, query, &NAMES)?
            };
            let row = Row {
                label,
                detail: String::new(),
                icon,
                choice: Choice::Author(name.to_owned()),
                checked: checked(name),
                lit,
            };
            Some((score, row))
        };
        if let Some(me) = &lists.me
            && let Some((_, row)) = matched(me, tr!("git.filter_me", name = me.as_str()), "user")
        {
            rows.push(row);
        }
        let mut found: Vec<(i32, Row)> =
            lists.authors.iter().filter_map(|a| matched(a, a.clone(), "user")).collect();
        if !query.is_empty() {
            found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
        }
        rows.extend(found.into_iter().map(|(_, r)| r).take(LIST_ROWS));
        rows
    }

    /// The folders and files git tracks (those chosen first), matched by the search: at
    /// most [`LIST_ROWS`], and how many more matched.
    /// The Paths list's rows for `query`, matched in the background (the folders and files
    /// are thousands: the typing never waits for them); what's there stays until they come.
    fn match_paths(&mut self, query: Vec<char>) {
        let Some(lists) = self.bar.lists.clone() else { return };
        self.bar.paths_generation += 1;
        let generation = self.bar.paths_generation;
        let chosen = self.filter.paths.clone();
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-log", move || {
            let (rows, more) = path_choices(&lists, &chosen, &query);
            if tx.send((generation, rows, more)).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.bar.paths_matching = Some(rx);
    }

    /// The Paths list's rows matched in the background, if they came (and are still the
    /// latest search's, its list still open). `true` if they were taken.
    pub(super) fn poll_paths(&mut self) -> bool {
        let Some(rx) = &self.bar.paths_matching else { return false };
        let Ok((generation, rows, more)) = rx.try_recv() else { return false };
        self.bar.paths_matching = None;
        let current = generation == self.bar.paths_generation;
        match &mut self.bar.popup {
            Some(popup) if current && popup.kind == Some(FilterKind::Paths) => {
                popup.active = popup.active.filter(|&a| a < rows.len());
                popup.rows = rows;
                popup.more = more;
                true
            }
            _ => false,
        }
    }

    fn order_choices(&self) -> Vec<Row> {
        [
            (LogOrder::Date, "git.order_date", "calendar"),
            (LogOrder::Topological, "git.order_topo", "graph"),
        ]
        .into_iter()
        .map(|(order, key, icon)| Row {
            label: i18n::t(key),
            detail: String::new(),
            icon,
            choice: Choice::Order(order),
            checked: self.filter.order == order,
            lit: Vec::new(),
        })
        .collect()
    }

    /// Row `i` of the open list chosen: a branch, an author or a path taken or left (the
    /// list stays open), a time or an order set (it closes).
    pub(super) fn choose_row(&mut self, i: usize) {
        let Some(row) = self.bar.popup.as_ref().and_then(|p| p.rows.get(i)).cloned() else {
            return;
        };
        let mut filter = self.filter.clone();
        let toggle = |list: &mut Vec<String>, value: String| {
            if let Some(at) = list.iter().position(|v| *v == value) {
                list.remove(at);
            } else {
                list.push(value);
            }
        };
        match row.choice {
            Choice::Header => return,
            Choice::Reference(name) => toggle(&mut filter.references, name),
            Choice::Author(name) => toggle(&mut filter.authors, name),
            Choice::Path(path) => toggle(&mut filter.paths, path),
            Choice::Order(order) => {
                filter.order = order;
                self.close_popup();
            }
        }
        self.apply_filter(filter);
        self.refresh_rows();
    }

    /// What the Date picker asked: a preset (applied at once: Any time clears the filter),
    /// a range applied, or nothing (cancelled); it closes.
    pub(super) fn picker_event(&mut self, event: ui_components::PickerEvent) {
        use ui_components::PickerEvent;
        let picker = self.bar.picker.take();
        match event {
            PickerEvent::Preset(i) => {
                let Some(preset) = picker.as_ref().and_then(|p| p.presets.get(i)).cloned() else {
                    return;
                };
                match preset.range {
                    Some(range) => self.set_dates(Some(DateChoice::Preset(preset.label, range))),
                    None => self.set_dates(None),
                }
            }
            PickerEvent::Apply(a, b) => self.set_dates(Some(DateChoice::Range(a, b))),
            PickerEvent::Cancel => {}
        }
    }

    /// The Date filter: from the first day's local midnight to the end of the last one.
    pub(super) fn set_dates(&mut self, date: Option<DateChoice>) {
        let (since, until) = match date.as_ref().map(DateChoice::range) {
            Some((a, b)) => {
                (crate::local_time::date_start(a), crate::local_time::date_start(b.add_days(1)))
            }
            None => (None, None),
        };
        self.bar.date = date;
        let filter = LogFilter { since, until, ..self.filter.clone() };
        self.apply_filter(filter);
    }

    /// Filter `kind` cleared (its pill's ×).
    pub(super) fn clear_filter(&mut self, kind: FilterKind) {
        let mut filter = self.filter.clone();
        match kind {
            FilterKind::Branch => filter.references.clear(),
            FilterKind::User => filter.authors.clear(),
            FilterKind::Date => {
                self.bar.date = None;
                filter.since = None;
                filter.until = None;
            }
            FilterKind::Paths => filter.paths.clear(),
            FilterKind::Order => filter.order = LogOrder::Date,
        }
        self.apply_filter(filter);
        self.refresh_rows();
    }

    /// What filter `kind`'s pill says, and whether it's set (lit, with its ×).
    pub(super) fn pill_label(&self, kind: FilterKind) -> (String, bool) {
        let f = &self.filter;
        let list = |title: &str, values: &[String], show: &dyn Fn(&str) -> String| match values {
            [] => (i18n::t(title), false),
            [one] => (format!("{}: {}", i18n::t(title), show(one)), true),
            [one, rest @ ..] => {
                (format!("{}: {} +{}", i18n::t(title), show(one), rest.len()), true)
            }
        };
        match kind {
            FilterKind::Branch => list("git.filter_branch", &f.references, &|s| s.to_owned()),
            FilterKind::User => list("git.filter_user", &f.authors, &|s| s.to_owned()),
            FilterKind::Paths => list("git.filter_paths", &f.paths, &|s| {
                s.rsplit('/').next().unwrap_or(s).to_owned()
            }),
            FilterKind::Date => match &self.bar.date {
                None => (i18n::t("git.filter_date"), false),
                Some(DateChoice::Preset(label, _)) => {
                    (format!("{}: {label}", i18n::t("git.filter_date")), true)
                }
                Some(DateChoice::Range(a, b)) => {
                    let locale = ui_components::DateLocale::current();
                    let range = locale.range((*a, *b), crate::local_time::today());
                    (format!("{}: {range}", i18n::t("git.filter_date")), true)
                }
            },
            FilterKind::Order => match f.order {
                LogOrder::Date => (i18n::t("git.order_date"), false),
                LogOrder::Topological => (i18n::t("git.order_topo"), true),
            },
        }
    }

    /// The filters set, as a line (under "Nothing found").
    pub(super) fn filters_summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(text) = &self.filter.text {
            parts.push(format!("\u{201c}{}\u{201d}", text.text));
        }
        for kind in [FilterKind::Branch, FilterKind::User, FilterKind::Date, FilterKind::Paths] {
            let (label, set) = self.pill_label(kind);
            if set {
                parts.push(label);
            }
        }
        parts.join(" · ")
    }

    /// A branch's row of the panel chosen twice (a double click): the Branch filter is that
    /// branch alone ("All": none).
    pub(super) fn filter_branch(&mut self, reference: Option<String>) {
        let filter =
            LogFilter { references: reference.into_iter().collect(), ..self.filter.clone() };
        self.apply_filter(filter);
    }
}

fn header(label: &str) -> Row {
    Row {
        label: label.to_owned(),
        detail: String::new(),
        icon: "",
        choice: Choice::Header,
        checked: false,
        lit: Vec::new(),
    }
}

/// The Paths list's rows for `query` (`chosen` first, lit): the folders, then the files, the
/// best matches only built into rows (at most [`LIST_ROWS`]); with how many more match.
pub(super) fn path_choices(lists: &Lists, chosen: &[String], query: &[char]) -> (Vec<Row>, usize) {
    let entries = lists
        .folders
        .iter()
        .map(|f| (f.as_str(), true))
        .chain(lists.files.iter().map(|f| (f.as_str(), false)));
    let mut found: Vec<(i32, &str, bool, Vec<usize>)> = entries
        .filter(|(path, _)| !chosen.iter().any(|c| c == path))
        .filter_map(|(path, folder)| {
            let (score, lit) = if query.is_empty() {
                // Unsearched: the folders, then the files at the top.
                if !folder && path.contains('/') {
                    return None;
                }
                (0, Vec::new())
            } else {
                kernel::fuzzy_match(path, query, &NAMES)?
            };
            Some((score, path, folder, lit))
        })
        .collect();
    if !query.is_empty() {
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
    }
    let more = found.len().saturating_sub(LIST_ROWS);
    let mut rows: Vec<Row> = chosen
        .iter()
        .map(|p| path_row(p, !lists.files.iter().any(|f| f == p), true, Vec::new()))
        .collect();
    rows.extend(
        found
            .into_iter()
            .take(LIST_ROWS)
            .map(|(_, path, folder, lit)| path_row(path, folder, false, lit)),
    );
    (rows, more)
}

fn path_row(path: &str, folder: bool, checked: bool, lit: Vec<usize>) -> Row {
    Row {
        label: path.to_owned(),
        detail: String::new(),
        icon: if folder { "folder" } else { "file" },
        choice: Choice::Path(path.to_owned()),
        checked,
        lit,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/log_view/filters.rs"]
mod tests;
