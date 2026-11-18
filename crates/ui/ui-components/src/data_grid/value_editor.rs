//! How a cell's value is edited, chosen by its column: an extension point
//! ([`ValueEditorProvider`]) the modules contribute to (a driver's own types: Postgres's
//! arrays, an enum…), the built-in editors ([`builtins`]: text, numbers, bool, date / time,
//! JSON) always there under them. The provider fitting a column best wins
//! ([`ValueEditorProvider::fit`]: the highest, the built-ins at 0); it says what the editor
//! starts with, whether it opens in the large editor, and reads the text typed back as a
//! value (or says why it isn't one).
//!
//! A provider can be data alone ([`ValueEditorSpec`]: the columns it's for, a
//! [`ValueFormat`], large or not, its priority) — what a driver behind a process or a
//! component (37i) declares without running code here; the built-ins are such specs.

use std::sync::{Arc, PoisonError, RwLock};

use super::source::{CellClass, CellRef, CellValue, GridColumn};

/// How a value is read from the text typed and written back for editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueFormat {
    /// Any text, as typed.
    Text,
    Integer,
    Decimal,
    Bool,
    /// A date, a time or both (`YYYY-MM-DD`, `HH:MM[:SS[.fff]]`, joined by a space or `T`,
    /// an offset or `Z` after).
    DateTime,
    /// `YYYY-MM-DD` only.
    Date,
    /// `HH:MM[:SS[.fff]]` only.
    Time,
    /// JSON: shown pretty-printed, checked, kept compact.
    Json,
    /// One of these values (an enum's).
    Choice(Vec<String>),
    /// As written, unchecked (a decimal, a UUID, an engine's own type).
    Raw,
}

/// The column a value is edited for (what a provider looks at).
pub type EditTarget = GridColumn;

/// A value editor (see the module's doc). Everything has a default but [`Self::id`] and
/// [`Self::fit`].
pub trait ValueEditorProvider: Send + Sync + std::fmt::Debug {
    /// Its id (`json`, `postgres.array`).
    fn id(&self) -> &str;
    /// How well it fits `column` (`None`: not at all); the highest wins, the built-ins
    /// give 0.
    fn fit(&self, column: &EditTarget) -> Option<i32>;
    /// What the editor starts with for `value` (`NULL`: empty).
    fn text(&self, value: &CellRef<'_>, column: &EditTarget) -> String {
        let _ = column;
        match value {
            CellRef::Null => String::new(),
            other => other.display(),
        }
    }
    /// The text typed as a value for `column` (blank in a nullable column of a type
    /// without text: `NULL`). `Err`: why it isn't one, for the user.
    fn parse(&self, text: &str, column: &EditTarget) -> Result<CellValue, String>;
    /// Edited in the large editor (several lines) rather than in the cell.
    fn large(&self, column: &EditTarget) -> bool {
        let _ = column;
        false
    }
    /// `new` is what `old` already was (written the same way or not): nothing changes.
    fn same(&self, old: &CellRef<'_>, new: &CellValue) -> bool {
        old.to_value() == *new
    }
}

/// A provider as data (see the module's doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueEditorSpec {
    pub id: String,
    /// The columns' classes it's for (none: any).
    pub classes: Vec<CellClass>,
    /// The types it's for, by name, any case, with or without what follows in
    /// parentheses (`json`, `varchar` for `varchar(255)`; none: any). A type named fits
    /// better than a class.
    pub types: Vec<String>,
    pub format: ValueFormat,
    pub large: bool,
    /// Added to how well it fits (a driver's own over the built-ins: above 0).
    pub priority: i32,
}

impl ValueEditorSpec {
    pub fn new(id: impl Into<String>, format: ValueFormat) -> Self {
        Self {
            id: id.into(),
            classes: Vec::new(),
            types: Vec::new(),
            format,
            large: false,
            priority: 0,
        }
    }

    pub fn for_classes(mut self, classes: &[CellClass]) -> Self {
        self.classes = classes.to_vec();
        self
    }

    pub fn for_types(mut self, types: &[&str]) -> Self {
        self.types = types.iter().map(|t| t.to_ascii_lowercase()).collect();
        self
    }

    pub fn in_large_editor(mut self) -> Self {
        self.large = true;
        self
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// One of its type names is `type_name` (the column's declared type, its size or
    /// what follows a space left out: `varchar(255)`, `time with time zone`).
    fn names_type(&self, type_name: &str) -> bool {
        let declared = type_name.trim().to_ascii_lowercase();
        let base = declared.split(['(', ' ']).next().unwrap_or_default();
        self.types.iter().any(|t| *t == declared || t == base)
    }
}

impl ValueEditorProvider for ValueEditorSpec {
    fn id(&self) -> &str {
        &self.id
    }

    fn fit(&self, column: &EditTarget) -> Option<i32> {
        let class = self.classes.is_empty() || self.classes.contains(&column.class);
        let named = !self.types.is_empty() && self.names_type(&column.type_name);
        // A type named counts more than its class; one not named, when some are, doesn't
        // fit (unless the class does and no type is required).
        match (named, class, self.types.is_empty()) {
            (true, _, _) => Some(self.priority + 2),
            (false, true, true) => {
                Some(self.priority + if self.classes.is_empty() { 0 } else { 1 })
            }
            _ => None,
        }
    }

    fn text(&self, value: &CellRef<'_>, _column: &EditTarget) -> String {
        match (value, &self.format) {
            (CellRef::Null, _) => String::new(),
            (CellRef::Json(t) | CellRef::Text(t) | CellRef::Other(t), ValueFormat::Json) => {
                json::format(t, true).unwrap_or_else(|_| (*t).to_owned())
            }
            (other, _) => other.display(),
        }
    }

    fn parse(&self, text: &str, column: &EditTarget) -> Result<CellValue, String> {
        read(&self.format, text, column.nullable)
    }

    fn large(&self, _column: &EditTarget) -> bool {
        self.large
    }

    fn same(&self, old: &CellRef<'_>, new: &CellValue) -> bool {
        match (&self.format, old, new) {
            // JSON written another way (spaces, line breaks) is the same JSON.
            (
                ValueFormat::Json,
                CellRef::Json(a) | CellRef::Text(a) | CellRef::Other(a),
                CellValue::Json(b),
            ) => json::format(a, false).ok().is_some_and(|a| a == *b),
            _ => old.to_value() == *new,
        }
    }
}

/// `text` read in `format` (`nullable`: blank is `NULL` for a value that isn't text).
pub fn read(format: &ValueFormat, text: &str, nullable: bool) -> Result<CellValue, String> {
    let trimmed = text.trim();
    let blank = trimmed.is_empty();
    let textual = matches!(format, ValueFormat::Text | ValueFormat::Raw);
    if blank && nullable && !textual {
        return Ok(CellValue::Null);
    }
    match format {
        ValueFormat::Text => Ok(CellValue::Text(text.to_owned())),
        ValueFormat::Raw => Ok(CellValue::Other(text.to_owned())),
        ValueFormat::Integer => trimmed
            .parse::<i64>()
            .map(CellValue::Int)
            .map_err(|_| i18n::tr!("grid.not_integer", value = trimmed)),
        ValueFormat::Decimal => trimmed
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(CellValue::Float)
            .ok_or_else(|| i18n::tr!("grid.not_number", value = trimmed)),
        ValueFormat::Bool => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" | "y" => Ok(CellValue::Bool(true)),
            "false" | "f" | "0" | "no" | "n" => Ok(CellValue::Bool(false)),
            _ => Err(i18n::tr!("grid.not_bool", value = trimmed)),
        },
        ValueFormat::DateTime | ValueFormat::Date | ValueFormat::Time => {
            let (date, time) = match format {
                ValueFormat::Date => (true, false),
                ValueFormat::Time => (false, true),
                _ => (true, true),
            };
            if datetime::valid(trimmed, date, time) {
                Ok(CellValue::Text(trimmed.to_owned()))
            } else {
                let key = match format {
                    ValueFormat::Date => "grid.not_date",
                    ValueFormat::Time => "grid.not_time",
                    _ => "grid.not_datetime",
                };
                Err(i18n::tr!(key, value = trimmed))
            }
        }
        ValueFormat::Json => match json::format(text, false) {
            Ok(compact) => Ok(CellValue::Json(compact)),
            Err(at) => Err(i18n::tr!("grid.not_json", at = at + 1)),
        },
        ValueFormat::Choice(values) => values
            .iter()
            .find(|v| v.as_str() == trimmed)
            .or_else(|| values.iter().find(|v| v.eq_ignore_ascii_case(trimmed)))
            .map(|v| CellValue::Text(v.clone()))
            .ok_or_else(|| {
                i18n::tr!("grid.not_choice", value = trimmed, choices = values.join(", "))
            }),
    }
}

/// The built-in editors (at priority 0): text, whole and decimal numbers, bool, date /
/// time (a type named `date` or `time` checked as such), JSON (in the large editor), and
/// anything else as written.
pub fn builtins() -> Vec<ValueEditorSpec> {
    use CellClass as C;
    vec![
        ValueEditorSpec::new("text", ValueFormat::Text).for_classes(&[C::Text]),
        ValueEditorSpec::new("integer", ValueFormat::Integer).for_classes(&[C::Integer]),
        ValueEditorSpec::new("decimal", ValueFormat::Decimal).for_classes(&[C::Decimal]),
        ValueEditorSpec::new("bool", ValueFormat::Bool).for_classes(&[C::Bool]),
        ValueEditorSpec::new("datetime", ValueFormat::DateTime).for_classes(&[C::DateTime]),
        // A type named `date` or `time` (not `datetime`, `timestamp`): checked as such.
        ValueEditorSpec::new("date", ValueFormat::Date).for_types(&["date"]),
        ValueEditorSpec::new("time", ValueFormat::Time).for_types(&["time"]),
        ValueEditorSpec::new("json", ValueFormat::Json).for_classes(&[C::Json]).in_large_editor(),
        ValueEditorSpec::new("raw", ValueFormat::Raw).for_classes(&[C::Other]),
    ]
}

/// The editors the modules contributed (registered by the app at startup).
static REGISTERED: RwLock<Vec<Arc<dyn ValueEditorProvider>>> = RwLock::new(Vec::new());

/// Registers a provider (at startup, from a module's contribution); one with the same id
/// is replaced.
pub fn register(provider: Arc<dyn ValueEditorProvider>) {
    let mut all = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    all.retain(|p| p.id() != provider.id());
    all.push(provider);
}

/// The provider fitting `column` best among `providers` (the first of equal ones).
pub fn choose(
    providers: &[Arc<dyn ValueEditorProvider>],
    column: &EditTarget,
) -> Option<Arc<dyn ValueEditorProvider>> {
    let mut best: Option<(i32, &Arc<dyn ValueEditorProvider>)> = None;
    for provider in providers {
        let Some(score) = provider.fit(column) else { continue };
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, provider));
        }
    }
    best.map(|(_, p)| Arc::clone(p))
}

/// The editor for `column`: the registered providers first (a tie goes to them), then the
/// built-ins; text as written when none fits.
pub fn editor_for(column: &EditTarget) -> Arc<dyn ValueEditorProvider> {
    let mut all: Vec<Arc<dyn ValueEditorProvider>> =
        REGISTERED.read().unwrap_or_else(PoisonError::into_inner).clone();
    all.extend(builtins().into_iter().map(|s| Arc::new(s) as Arc<dyn ValueEditorProvider>));
    choose(&all, column).unwrap_or_else(|| Arc::new(ValueEditorSpec::new("raw", ValueFormat::Raw)))
}

/// Dates and times as typed, checked without a calendar library.
mod datetime {
    fn digits(s: &str, n: usize) -> Option<u32> {
        (s.len() == n && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
    }

    fn number(s: &str, len: std::ops::RangeInclusive<usize>) -> Option<u32> {
        (len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
    }

    fn days(y: u32, m: u32) -> u32 {
        let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        }
    }

    /// A date: `YYYY-MM-DD` (also with `/` or `.`), or the year last (`05.03.2024`,
    /// `03/05/2024`: day or month first, as a CSV file writes it).
    fn date(s: &str) -> bool {
        let valid = |y: u32, m: u32, d: u32| y > 0 && d >= 1 && d <= days(y, m);
        for sep in ['-', '/', '.'] {
            let parts: Vec<&str> = s.split(sep).collect();
            let [a, b, c] = parts.as_slice() else { continue };
            if let (Some(y), Some(m), Some(d)) =
                (number(a, 4..=4), number(b, 1..=2), number(c, 1..=2))
            {
                return valid(y, m, d);
            }
            if let (Some(x), Some(z), Some(y)) =
                (number(a, 1..=2), number(b, 1..=2), number(c, 4..=4))
            {
                return valid(y, x, z) || valid(y, z, x);
            }
        }
        false
    }

    fn time(s: &str) -> bool {
        // An offset or Z after it.
        let s = s.strip_suffix('Z').or_else(|| s.strip_suffix('z')).unwrap_or(s);
        let s = match s.rfind(['+', '-']) {
            Some(at) if at >= 5 => {
                let offset = &s[at + 1..];
                let ok = match offset.split_once(':') {
                    Some((h, m)) => {
                        digits(h, 2).is_some_and(|h| h <= 14)
                            && digits(m, 2).is_some_and(|m| m < 60)
                    }
                    None => {
                        digits(offset, 2).is_some_and(|h| h <= 14)
                            || digits(offset, 4).is_some_and(|hm| hm / 100 <= 14 && hm % 100 < 60)
                    }
                };
                if !ok {
                    return false;
                }
                &s[..at]
            }
            _ => s,
        };
        let (clock, fraction) = match s.split_once('.') {
            Some((c, f)) => (c, Some(f)),
            None => (s, None),
        };
        if fraction
            .is_some_and(|f| f.is_empty() || f.len() > 9 || !f.bytes().all(|b| b.is_ascii_digit()))
        {
            return false;
        }
        let parts: Vec<&str> = clock.split(':').collect();
        let limits = [24, 60, 61];
        (2..=3).contains(&parts.len())
            && (fraction.is_none() || parts.len() == 3)
            && parts.iter().zip(limits).all(|(p, max)| digits(p, 2).is_some_and(|v| v < max))
    }

    /// `s` is a date (`with_date`), a time (`with_time`), or both joined by a space or `T`
    /// (then a date alone is one too).
    pub fn valid(s: &str, with_date: bool, with_time: bool) -> bool {
        match (with_date, with_time) {
            (true, false) => date(s),
            (false, true) => time(s),
            _ => match s.split_once([' ', 'T', 't']) {
                Some((d, t)) => date(d) && time(t.trim_start()),
                None => date(s) || time(s),
            },
        }
    }
}

/// JSON checked and written again (pretty or compact) without building it: what's written
/// keeps its keys' order and its numbers as typed.
pub mod json {
    /// `text` written again, `pretty` (two spaces) or compact; `Err`: the character where
    /// it stops being JSON (0-based).
    pub fn format(text: &str, pretty: bool) -> Result<String, usize> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut p = Parser { chars: &chars, at: 0, out: String::with_capacity(text.len()), pretty };
        p.space();
        p.value(0)?;
        p.space();
        if p.at < chars.len() {
            return Err(p.at);
        }
        Ok(p.out)
    }

    struct Parser<'a> {
        chars: &'a [(usize, char)],
        at: usize,
        out: String,
        pretty: bool,
    }

    impl Parser<'_> {
        fn peek(&self) -> Option<char> {
            self.chars.get(self.at).map(|(_, c)| *c)
        }

        fn space(&mut self) {
            while self.peek().is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
                self.at += 1;
            }
        }

        fn eat(&mut self, c: char) -> Result<(), usize> {
            if self.peek() == Some(c) {
                self.at += 1;
                Ok(())
            } else {
                Err(self.at)
            }
        }

        fn newline(&mut self, depth: usize) {
            if self.pretty {
                self.out.push('\n');
                for _ in 0..depth {
                    self.out.push_str("  ");
                }
            }
        }

        fn value(&mut self, depth: usize) -> Result<(), usize> {
            if depth > 512 {
                return Err(self.at);
            }
            match self.peek() {
                Some('{') => self.container(depth, '{', '}', true),
                Some('[') => self.container(depth, '[', ']', false),
                Some('"') => self.string(),
                Some('-' | '0'..='9') => self.number(),
                Some('t') => self.word("true"),
                Some('f') => self.word("false"),
                Some('n') => self.word("null"),
                _ => Err(self.at),
            }
        }

        fn container(
            &mut self,
            depth: usize,
            open: char,
            close: char,
            object: bool,
        ) -> Result<(), usize> {
            self.eat(open)?;
            self.out.push(open);
            self.space();
            if self.peek() == Some(close) {
                self.at += 1;
                self.out.push(close);
                return Ok(());
            }
            loop {
                self.newline(depth + 1);
                if object {
                    self.space();
                    if self.peek() != Some('"') {
                        return Err(self.at);
                    }
                    self.string()?;
                    self.space();
                    self.eat(':')?;
                    self.out.push(':');
                    if self.pretty {
                        self.out.push(' ');
                    }
                    self.space();
                }
                self.value(depth + 1)?;
                self.space();
                match self.peek() {
                    Some(',') => {
                        self.at += 1;
                        self.out.push(',');
                        self.space();
                    }
                    Some(c) if c == close => {
                        self.at += 1;
                        self.newline(depth);
                        self.out.push(close);
                        return Ok(());
                    }
                    _ => return Err(self.at),
                }
            }
        }

        fn string(&mut self) -> Result<(), usize> {
            self.eat('"')?;
            self.out.push('"');
            loop {
                let Some(c) = self.peek() else { return Err(self.at) };
                self.at += 1;
                match c {
                    '"' => {
                        self.out.push('"');
                        return Ok(());
                    }
                    '\\' => {
                        let Some(e) = self.peek() else { return Err(self.at) };
                        self.at += 1;
                        self.out.push('\\');
                        self.out.push(e);
                        match e {
                            '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {}
                            'u' => {
                                for _ in 0..4 {
                                    match self.peek() {
                                        Some(h) if h.is_ascii_hexdigit() => {
                                            self.out.push(h);
                                            self.at += 1;
                                        }
                                        _ => return Err(self.at),
                                    }
                                }
                            }
                            _ => return Err(self.at - 1),
                        }
                    }
                    c if (c as u32) < 0x20 => return Err(self.at - 1),
                    c => self.out.push(c),
                }
            }
        }

        fn digits(&mut self) -> usize {
            let start = self.at;
            while let Some(c) = self.peek().filter(char::is_ascii_digit) {
                self.out.push(c);
                self.at += 1;
            }
            self.at - start
        }

        fn number(&mut self) -> Result<(), usize> {
            if self.peek() == Some('-') {
                self.out.push('-');
                self.at += 1;
            }
            let lead = self.peek();
            let whole = self.digits();
            if whole == 0 || (lead == Some('0') && whole > 1) {
                return Err(self.at);
            }
            if self.peek() == Some('.') {
                self.out.push('.');
                self.at += 1;
                if self.digits() == 0 {
                    return Err(self.at);
                }
            }
            if let Some(e @ ('e' | 'E')) = self.peek() {
                self.out.push(e);
                self.at += 1;
                if let Some(s @ ('+' | '-')) = self.peek() {
                    self.out.push(s);
                    self.at += 1;
                }
                if self.digits() == 0 {
                    return Err(self.at);
                }
            }
            Ok(())
        }

        fn word(&mut self, word: &str) -> Result<(), usize> {
            for w in word.chars() {
                self.eat(w)?;
            }
            self.out.push_str(word);
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/value_editor.rs"]
mod tests;
