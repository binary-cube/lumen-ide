//! A CSV file's dialect and its columns' types, guessed from a sample of its start (pure:
//! the bytes are given): the delimiter (`,` `;` tab `|`), quoting, a byte order mark, a
//! header, and each column's class (integer, decimal, bool, date / time, or text). Also how
//! a file is read and written a record at a time, by the same rules: where its records end
//! in a stream of bytes ([`RecordScanner`], a quoted line break being a field's), a record's
//! fields as written ([`split_fields`], [`field_text`]), how it writes them ([`CsvStyle`],
//! [`encode_field`]) and its cells as values ([`cell_value`], [`cell_text`]).

use std::ops::Range;

use crate::{TypeClass, Value};

/// The delimiters tried, in order of preference on a tie.
pub const DELIMITERS: [u8; 4] = *b",;\t|";

/// UTF-8's byte order mark.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// How a CSV file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvDialect {
    pub delimiter: u8,
    /// Fields holding the delimiter, quotes or line breaks are between these (doubled
    /// inside).
    pub quote: u8,
    /// Its first record names the columns.
    pub header: bool,
    /// It starts with UTF-8's byte order mark (kept when it's written back).
    pub bom: bool,
}

impl Default for CsvDialect {
    fn default() -> Self {
        Self { delimiter: b',', quote: b'"', header: true, bom: false }
    }
}

/// A column, as guessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvColumn {
    /// From the header (`None`: none, the UI numbers it).
    pub name: Option<String>,
    pub class: TypeClass,
}

/// What a sample says of its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvGuess {
    pub dialect: CsvDialect,
    pub columns: Vec<CsvColumn>,
}

/// `bytes` without a leading byte order mark, and whether there was one.
pub fn strip_bom(bytes: &[u8]) -> (&[u8], bool) {
    match bytes.strip_prefix(BOM) {
        Some(rest) => (rest, true),
        None => (bytes, false),
    }
}

/// The records of `text` (RFC 4180: quoted fields with doubled quotes and line breaks
/// inside, `\n` or `\r\n` between records), at most `limit`. `complete`: `text` is the
/// whole file — otherwise its last record may be cut and is left out. Empty lines are
/// skipped.
pub fn parse(
    text: &str,
    delimiter: u8,
    quote: u8,
    limit: usize,
    complete: bool,
) -> Vec<Vec<String>> {
    records(text, delimiter, quote, limit, complete).0
}

/// [`parse`]'s records, and how many characters followed a closing quote (a sign of the
/// wrong delimiter: `"a,b";"c"` split by commas).
fn records(
    text: &str,
    delimiter: u8,
    quote: u8,
    limit: usize,
    complete: bool,
) -> (Vec<Vec<String>>, usize) {
    let (delimiter, quote) = (char::from(delimiter), char::from(quote));
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    // The field was quoted (`""` is an empty field, not none).
    let mut was_quoted = false;
    let mut stray = 0;
    let mut chars = text.chars().peekable();
    let push_record = |record: &mut Vec<String>, records: &mut Vec<Vec<String>>| {
        let empty_line = record.len() == 1 && record[0].is_empty();
        if !empty_line {
            records.push(std::mem::take(record));
        }
        record.clear();
    };
    while let Some(c) = chars.next() {
        if records.len() >= limit {
            return (records, stray);
        }
        if quoted {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    chars.next();
                    field.push(quote);
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            c if c == quote && field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            c if c == delimiter => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
                push_record(&mut record, &mut records);
            }
            c => {
                if was_quoted {
                    stray += 1;
                }
                field.push(c);
            }
        }
    }
    if complete && !quoted && (!field.is_empty() || !record.is_empty() || was_quoted) {
        record.push(field);
        push_record(&mut record, &mut records);
    }
    records.truncate(limit);
    (records, stray)
}

/// The delimiter `text` is written with: the one leaving no text after a closing quote,
/// splitting its records into the same number (two or more) of fields most often, then into
/// the most fields (`,` when none does).
pub fn sniff_delimiter(text: &str, quote: u8, complete: bool) -> u8 {
    const SAMPLE: usize = 64;
    let mut best: Option<(u8, usize, f64, usize)> = None;
    for delimiter in DELIMITERS {
        let (records, stray) = records(text, delimiter, quote, SAMPLE, complete);
        if records.is_empty() {
            continue;
        }
        let mut counts: Vec<usize> = records.iter().map(Vec::len).collect();
        counts.sort_unstable();
        // The most frequent count (the largest on a tie).
        let (mut mode, mut freq, mut run) = (0, 0, 0);
        for (i, &n) in counts.iter().enumerate() {
            run = if i > 0 && counts[i - 1] == n { run + 1 } else { 1 };
            if run >= freq {
                (mode, freq) = (n, run);
            }
        }
        if mode < 2 {
            continue;
        }
        let consistency = freq as f64 / records.len() as f64;
        let better = match best {
            None => true,
            Some((_, s, c, m)) => {
                stray < s
                    || (stray == s
                        && (consistency > c + 1e-9
                            || ((consistency - c).abs() <= 1e-9 && mode > m)))
            }
        };
        if better {
            best = Some((delimiter, stray, consistency, mode));
        }
    }
    best.map_or(b',', |(d, ..)| d)
}

/// `cell` is an integer (no leading zeros: `007` is a code, kept as text).
fn is_integer(cell: &str) -> bool {
    let digits = cell.strip_prefix(['+', '-']).unwrap_or(cell);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && cell.parse::<i64>().is_ok()
}

/// `cell` is a decimal number (`3.14`, `-.5`, `1e-3`; `3,14` when `comma` is decimal).
fn is_decimal(cell: &str, comma: bool) -> bool {
    let s = cell.strip_prefix(['+', '-']).unwrap_or(cell);
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(at) => (&s[..at], Some(&s[at + 1..])),
        None => (s, None),
    };
    let point = |c: char| c == '.' || (comma && c == ',');
    let mut parts = mantissa.splitn(2, point);
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = digits(whole)
        && (whole.len() <= 1 || !whole.starts_with('0'))
        && fraction.is_none_or(digits)
        && (!whole.is_empty() || fraction.is_some_and(|f| !f.is_empty()));
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

fn is_bool(cell: &str) -> bool {
    ["true", "false", "yes", "no"].iter().any(|b| cell.eq_ignore_ascii_case(b))
}

/// The number `text` spells, if it's `len` digits.
fn number(text: &str, len: std::ops::RangeInclusive<usize>) -> Option<u32> {
    (len.contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn is_date(text: &str) -> bool {
    let valid = |y: u32, m: u32, d: u32| (1..=12).contains(&m) && (1..=31).contains(&d) && y > 0;
    for sep in ['-', '/', '.'] {
        let parts: Vec<&str> = text.split(sep).collect();
        if let [a, b, c] = parts.as_slice() {
            // Year first (`2024-03-05`), or last (`05.03.2024`, `03/05/2024`: day or month
            // first, either is a date).
            if let (Some(y), Some(m), Some(d)) =
                (number(a, 4..=4), number(b, 1..=2), number(c, 1..=2))
                && valid(y, m, d)
            {
                return true;
            }
            if let (Some(x), Some(z), Some(y)) =
                (number(a, 1..=2), number(b, 1..=2), number(c, 4..=4))
                && (valid(y, x, z) || valid(y, z, x))
            {
                return true;
            }
        }
    }
    false
}

fn is_time(text: &str) -> bool {
    // An offset or `Z` after it.
    let text = text.strip_suffix(['Z', 'z']).unwrap_or(text);
    let text = match text.rfind(['+', '-']) {
        Some(at) if at > 0 => &text[..at],
        _ => text,
    };
    let (clock, fraction) = match text.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };
    if fraction.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    let field = |t: &str, max: u32| number(t, 2..=2).is_some_and(|n| n <= max);
    match parts.as_slice() {
        [h, m] => field(h, 23) && field(m, 59) && fraction.is_none(),
        [h, m, s] => field(h, 23) && field(m, 59) && field(s, 60),
        _ => false,
    }
}

/// `cell` is a date, a time, or both (`2024-03-05`, `05.03.2024`, `14:30`,
/// `2024-03-05T14:30:00.123Z`, `2024-03-05 14:30:00+02:00`).
fn is_date_time(cell: &str) -> bool {
    match cell.split_once(['T', ' ']) {
        Some((date, time)) => is_date(date) && is_time(time),
        None => is_date(cell) || is_time(cell),
    }
}

/// One cell's class (`None`: empty, any class takes it).
fn class_of(cell: &str, comma: bool) -> Option<TypeClass> {
    let cell = cell.trim();
    Some(if cell.is_empty() {
        return None;
    } else if is_integer(cell) {
        TypeClass::Integer
    } else if is_decimal(cell, comma) {
        TypeClass::Decimal
    } else if is_bool(cell) {
        TypeClass::Bool
    } else if is_date_time(cell) {
        TypeClass::DateTime
    } else {
        TypeClass::Text
    })
}

/// The class all of a column's cells fit (integers among decimals are decimals); text when
/// they don't agree, or all are empty.
fn merge(a: Option<TypeClass>, b: Option<TypeClass>) -> Option<TypeClass> {
    use TypeClass::{Decimal, Integer, Text};
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(Integer), Some(Decimal)) | (Some(Decimal), Some(Integer)) => Some(Decimal),
        _ => Some(Text),
    }
}

/// Each column's class over `rows` (a record's missing cells count as empty). `delimiter`:
/// a decimal comma is read only when the comma isn't the delimiter.
pub fn infer_types(rows: &[Vec<String>], delimiter: u8) -> Vec<TypeClass> {
    let comma = delimiter != b',';
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    (0..width)
        .map(|i| {
            rows.iter()
                .map(|r| r.get(i).and_then(|c| class_of(c, comma)))
                .fold(None, merge)
                .unwrap_or(TypeClass::Text)
        })
        .collect()
}

/// The first record names the columns: its cells are unlike the rest's (text over numbers,
/// dates or booleans; another length than the rest's all-equal ones); with nothing to
/// tell, when its cells are all filled, different and none a number or a date.
pub fn guess_header(records: &[Vec<String>], delimiter: u8) -> bool {
    let Some((first, rest)) = records.split_first() else { return false };
    let comma = delimiter != b',';
    if rest.is_empty() {
        return first.iter().all(|c| class_of(c, comma) == Some(TypeClass::Text));
    }
    let body = infer_types(rest, delimiter);
    let mut votes = 0i32;
    for (i, cell) in first.iter().enumerate() {
        let class = body.get(i).copied().unwrap_or(TypeClass::Text);
        if class != TypeClass::Text {
            // A value over values; a name over them.
            let value = class_of(cell, comma).is_some_and(|c| c != TypeClass::Text);
            votes += if value { -1 } else { 1 };
            continue;
        }
        // Text: a column of one length (codes) whose first cell has another.
        let lengths: Vec<usize> =
            rest.iter().filter_map(|r| r.get(i)).map(|c| c.chars().count()).collect();
        if let Some(&len) = lengths.first()
            && lengths.len() > 1
            && lengths.iter().all(|&l| l == len)
        {
            votes += if cell.chars().count() == len { -1 } else { 1 };
        }
    }
    if votes != 0 {
        return votes > 0;
    }
    let mut seen = std::collections::HashSet::new();
    first.iter().all(|c| {
        !c.trim().is_empty() && seen.insert(c.trim()) && class_of(c, comma) == Some(TypeClass::Text)
    })
}

/// The dialect and columns of a file starting with `sample` (`complete`: the sample is the
/// whole file), from at most `rows` records.
pub fn guess(sample: &[u8], rows: usize, complete: bool) -> CsvGuess {
    guess_with(sample, rows, complete, None)
}

/// [`guess`], its delimiter given when it's known (a `.tsv` file's tab).
pub fn guess_with(sample: &[u8], rows: usize, complete: bool, delimiter: Option<u8>) -> CsvGuess {
    let (bytes, bom) = strip_bom(sample);
    // A sample cut inside a character loses only that character.
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => std::borrow::Cow::Borrowed(text),
        Err(e) if !complete && e.error_len().is_none() => {
            std::borrow::Cow::Owned(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => String::from_utf8_lossy(bytes),
    };
    let quote = b'"';
    let delimiter = delimiter.unwrap_or_else(|| sniff_delimiter(&text, quote, complete));
    let records = parse(&text, delimiter, quote, rows.saturating_add(1), complete);
    let header = guess_header(&records, delimiter);
    let body = if header { records.get(1..).unwrap_or(&[]) } else { &records[..] };
    let classes = infer_types(body, delimiter);
    let width = classes.len().max(records.first().map_or(0, Vec::len));
    let columns = (0..width)
        .map(|i| CsvColumn {
            name: records
                .first()
                .and_then(|r| r.get(i))
                .filter(|name| header && !name.trim().is_empty())
                .map(|name| name.trim().to_owned()),
            class: classes.get(i).copied().unwrap_or(TypeClass::Text),
        })
        .collect();
    CsvGuess { dialect: CsvDialect { delimiter, quote, header, bom }, columns }
}

/// How a file's records end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::CrLf => b"\r\n",
        }
    }
}

/// How a file writes its records, beyond its dialect: kept by the records written back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CsvStyle {
    pub line_ending: LineEnding,
    /// Every field is quoted (otherwise only those that must be).
    pub quote_all: bool,
}

/// Where a scan is in a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    /// At a field's start (a quote there opens it).
    FieldStart,
    /// In a field not quoted (a quote is a character).
    Unquoted,
    Quoted,
    /// A quote in a quoted field: doubled (a quote) or its end.
    QuoteInQuoted,
}

/// Finds where records end in a stream of bytes, fed in pieces of any size (a file read a
/// chunk at a time): at a line feed outside quotes, by [`parse`]'s rules — a quote opens a
/// field only at its start, a doubled one inside is a quote, a line break inside is the
/// field's.
#[derive(Debug, Clone)]
pub struct RecordScanner {
    delimiter: u8,
    quote: u8,
    state: Scan,
}

impl RecordScanner {
    pub fn new(delimiter: u8, quote: u8) -> Self {
        Self { delimiter, quote, state: Scan::FieldStart }
    }

    /// The place in `bytes` (the stream's next piece) of the line feed ending the record
    /// under way; `None`: it goes on past them (all taken).
    pub fn end(&mut self, bytes: &[u8]) -> Option<usize> {
        let (delimiter, quote) = (self.delimiter, self.quote);
        let mut at = 0;
        while at < bytes.len() {
            match self.state {
                Scan::Quoted => {
                    at += bytes[at..].iter().position(|&b| b == quote)? + 1;
                    self.state = Scan::QuoteInQuoted;
                }
                Scan::Unquoted => {
                    match bytes[at..].iter().position(|&b| b == delimiter || b == b'\n') {
                        Some(n) if bytes[at + n] == b'\n' => {
                            self.state = Scan::FieldStart;
                            return Some(at + n);
                        }
                        Some(n) => {
                            at += n + 1;
                            self.state = Scan::FieldStart;
                        }
                        None => return None,
                    }
                }
                Scan::FieldStart | Scan::QuoteInQuoted => {
                    let b = bytes[at];
                    at += 1;
                    if b == b'\n' {
                        self.state = Scan::FieldStart;
                        return Some(at - 1);
                    }
                    self.state = if b == delimiter {
                        Scan::FieldStart
                    } else if b == quote {
                        // Opening a field at its start; doubled inside one, a quote.
                        Scan::Quoted
                    } else {
                        // After a closing quote: the field goes on, not quoted.
                        Scan::Unquoted
                    };
                }
            }
        }
        None
    }

    /// Inside a quoted field (a stream cut here cuts a field).
    pub fn in_quotes(&self) -> bool {
        self.state == Scan::Quoted
    }

    /// Back to a record's start.
    pub fn reset(&mut self) {
        self.state = Scan::FieldStart;
    }
}

/// A record's bytes without its line ending (`\n` or `\r\n`).
pub fn trim_line_ending(record: &[u8]) -> &[u8] {
    let record = record.strip_suffix(b"\n").unwrap_or(record);
    record.strip_suffix(b"\r").unwrap_or(record)
}

/// A record is a blank line (none of [`parse`]'s records): nothing before its line ending.
pub fn is_blank(record: &[u8]) -> bool {
    trim_line_ending(record).is_empty()
}

/// The fields of `record` (without its line ending) as written, quotes included: their
/// places, split at the delimiters outside quotes (at least one, maybe empty).
pub fn split_fields(record: &[u8], delimiter: u8, quote: u8) -> Vec<Range<usize>> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut state = Scan::FieldStart;
    for (at, &b) in record.iter().enumerate() {
        state = match state {
            Scan::Quoted if b == quote => Scan::QuoteInQuoted,
            Scan::Quoted => Scan::Quoted,
            _ if b == delimiter => {
                fields.push(start..at);
                start = at + 1;
                Scan::FieldStart
            }
            Scan::FieldStart | Scan::QuoteInQuoted if b == quote => Scan::Quoted,
            _ => Scan::Unquoted,
        };
    }
    fields.push(start..record.len());
    fields
}

/// A field's text from its bytes as written: a quoted one without its quotes, a doubled
/// quote inside as one (what follows its closing quote kept, as [`parse`] keeps it).
pub fn field_text(raw: &[u8], quote: u8) -> String {
    let Some(inner) = raw.strip_prefix(&[quote]) else {
        return String::from_utf8_lossy(raw).into_owned();
    };
    let mut out = Vec::with_capacity(inner.len());
    let mut at = 0;
    while at < inner.len() {
        let b = inner[at];
        at += 1;
        if b != quote {
            out.push(b);
        } else if inner.get(at) == Some(&quote) {
            out.push(quote);
            at += 1;
        } else {
            // Closed: the rest is the field's, as it stands.
            out.extend_from_slice(&inner[at..]);
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The text a field is written as: quoted (quotes inside doubled) when the style quotes
/// all, or when it holds the delimiter, a quote or a line break, or starts or ends with a
/// space; as it is otherwise.
pub fn encode_field(text: &str, dialect: &CsvDialect, style: &CsvStyle) -> String {
    let (delimiter, quote) = (char::from(dialect.delimiter), char::from(dialect.quote));
    let must = text.contains([delimiter, quote, '\n', '\r'])
        || text.starts_with([' ', '\t'])
        || text.ends_with([' ', '\t']);
    if !(style.quote_all || must) {
        return text.to_owned();
    }
    let doubled = text.replace(quote, &format!("{quote}{quote}"));
    format!("{quote}{doubled}{quote}")
}

/// A record written from its fields' texts ([`encode_field`]), without its line ending. A
/// single empty field is quoted: unquoted, it would be a blank line, no record.
pub fn encode_record(fields: &[String], dialect: &CsvDialect, style: &CsvStyle) -> String {
    if let [only] = fields
        && only.is_empty()
    {
        let quote = char::from(dialect.quote);
        return format!("{quote}{quote}");
    }
    let delimiter = char::from(dialect.delimiter).to_string();
    let encoded: Vec<String> = fields.iter().map(|f| encode_field(f, dialect, style)).collect();
    encoded.join(&delimiter)
}

/// How the file starting with `sample` (after its byte order mark) writes its records: the
/// first one's line ending, and whether every field of its first records is quoted.
pub fn guess_style(sample: &[u8], dialect: &CsvDialect) -> CsvStyle {
    const RECORDS: usize = 20;
    let mut scanner = RecordScanner::new(dialect.delimiter, dialect.quote);
    let mut records: Vec<&[u8]> = Vec::new();
    let mut line_ending = None;
    let mut start = 0;
    while records.len() < RECORDS {
        let Some(n) = scanner.end(&sample[start..]) else { break };
        let end = start + n;
        let record = &sample[start..=end];
        if line_ending.is_none() {
            line_ending =
                Some(if record.ends_with(b"\r\n") { LineEnding::CrLf } else { LineEnding::Lf });
        }
        if !is_blank(record) {
            records.push(trim_line_ending(record));
        }
        start = end + 1;
    }
    if records.is_empty() && !sample[start..].is_empty() {
        records.push(&sample[start..]);
    }
    let quote_all = !records.is_empty()
        && records.iter().all(|record| {
            split_fields(record, dialect.delimiter, dialect.quote)
                .into_iter()
                .all(|f| record[f].first() == Some(&dialect.quote))
        });
    CsvStyle { line_ending: line_ending.unwrap_or_default(), quote_all }
}

/// A cell read as a value of its column's class: empty is `NULL` (a CSV has no other), an
/// integer column's integers numbers (written back the same), a text column's text, the
/// rest as written ([`Value::Other`]: decimals, dates, booleans keep their spelling).
pub fn cell_value(text: &str, class: TypeClass) -> Value {
    if text.is_empty() {
        return Value::Null;
    }
    match class {
        TypeClass::Integer => match text.parse::<i64>() {
            Ok(n) if n.to_string() == text => Value::Int(n),
            _ => Value::Other(text.to_owned()),
        },
        TypeClass::Text => Value::Text(text.to_owned()),
        _ => Value::Other(text.to_owned()),
    }
}

/// A value as a cell's text (`NULL`: empty); `None` for bytes (a CSV holds only text).
pub fn cell_text(value: &Value) -> Option<String> {
    match value {
        Value::Bytes(_) => None,
        other => Some(other.display()),
    }
}

#[cfg(test)]
#[path = "../tests/unit/csv.rs"]
mod tests;
