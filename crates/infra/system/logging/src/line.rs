//! One event as one line of JSON: when (UTC, to the millisecond), how grave, where from
//! (its target and its thread), the run it belongs to, its message and its fields with their
//! types. A duration (as `tracing` hands it, its `Debug` text: `12.3ms`) becomes a number of
//! milliseconds under the field's name with `_ms`. Secrets are hidden as each field is read
//! (`redact`). A line is never longer than [`MAX_LINE`]:
//! a long text is cut at a character's edge with `…`, its length noted.

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};

/// The longest a line may be (bytes).
pub const MAX_LINE: usize = 64 * 1024;

/// The longest one text in a line may be (bytes): a few such texts still fit a line.
pub const MAX_TEXT: usize = 16 * 1024;

/// An event's fields as read: its message apart.
#[derive(Debug, Default)]
pub struct Fields {
    pub message: Option<String>,
    pub fields: Map<String, Value>,
}

impl Fields {
    /// Field `name` is `value`: a secret's name hides it whole, a text's secrets of a known
    /// shape are hidden ([`crate::redact`]).
    fn put(&mut self, name: &str, value: Value) {
        let value = match value {
            _ if name != "message" && crate::redact::secret_name(name) => {
                Value::String(crate::redact::HIDDEN.to_owned())
            }
            Value::String(text) => Value::String(crate::redact::text(&text).into_owned()),
            other => other,
        };
        if name == "message" {
            self.message = Some(match value {
                Value::String(s) => s,
                other => other.to_string(),
            });
        } else {
            self.fields.insert(name.to_owned(), value);
        }
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let text = format!("{value:?}");
        match duration_ms(&text) {
            Some(ms) if field.name() != "message" => {
                self.put(&format!("{}_ms", field.name()), number(ms));
            }
            _ => self.put(field.name(), Value::String(cut(text))),
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), Value::String(cut(value.to_owned())));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), number(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), Value::Bool(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.put(field.name(), Value::String(cut(value.to_string())));
    }
}

/// A number JSON can hold (`NaN` and infinities are written as text).
fn number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map_or_else(|| Value::String(value.to_string()), Value::Number)
}

/// `text` as a duration's `Debug` (`1.5s`, `12.3ms`, `40µs`, `7ns`), in milliseconds.
pub fn duration_ms(text: &str) -> Option<f64> {
    let units: [(&str, f64); 5] =
        [("ns", 1e-6), ("µs", 1e-3), ("us", 1e-3), ("ms", 1.0), ("s", 1000.0)];
    let (number, scale) =
        units.iter().find_map(|(unit, scale)| text.strip_suffix(unit).map(|n| (n, *scale)))?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let value: f64 = number.parse().ok()?;
    // Three decimals are enough to read (a microsecond).
    Some((value * scale * 1000.0).round() / 1000.0)
}

/// `text` within [`MAX_TEXT`], cut at a character's edge with `…` and its length noted.
pub fn cut(text: String) -> String {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes)", &text[..end], text.len())
}

/// What a line says besides its fields.
#[derive(Debug, Clone, Copy)]
pub struct Head<'a> {
    /// Milliseconds since the epoch.
    pub at_ms: i64,
    pub level: &'a str,
    pub target: &'a str,
    pub thread: &'a str,
    pub run: &'a str,
}

/// The line (without its newline): `head`, then the message, the fields, the spans (each
/// its name and its fields, the outermost first). Never longer than [`MAX_LINE`].
pub fn line(head: Head<'_>, fields: Fields, spans: Vec<Value>) -> String {
    let message = fields.message.unwrap_or_default();
    let mut text = written(head, &message);
    // The keys in their order, the head first (serde's map would sort them).
    if !fields.fields.is_empty() {
        text.push_str(",\"fields\":");
        text.push_str(&Value::Object(fields.fields).to_string());
    }
    if !spans.is_empty() {
        text.push_str(",\"span\":");
        text.push_str(&Value::Array(spans).to_string());
    }
    text.push('}');
    if text.len() <= MAX_LINE {
        return text;
    }
    // Still too long (many long fields): only what says what it was, and how long it was.
    let mut short = written(head, &format!("a line of {} bytes, too long", text.len()));
    short.push('}');
    short
}

/// A line's start, its object still open: `ts`, `level`, `target`, `thread`, `run`, `msg`.
fn written(head: Head<'_>, message: &str) -> String {
    let quoted = |text: &str| Value::String(text.to_owned()).to_string();
    format!(
        "{{\"ts\":{},\"level\":{},\"target\":{},\"thread\":{},\"run\":{},\"msg\":{}",
        quoted(&timestamp(head.at_ms)),
        quoted(head.level),
        quoted(head.target),
        quoted(head.thread),
        quoted(head.run),
        quoted(message),
    )
}

/// `ms` since the epoch as RFC 3339 in UTC, to the millisecond (`2026-11-20T08:10:00.123Z`).
pub fn timestamp(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let (year, month, day) = civil(seconds.div_euclid(86_400));
    let in_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        in_day / 3600,
        in_day % 3600 / 60,
        in_day % 60
    )
}

/// The calendar day `days` after the epoch (UTC): year, month, day (Howard Hinnant's).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// The day of `ms` since the epoch, as a file names it (`2026-11-20`).
pub fn day_name(ms: i64) -> String {
    let (year, month, day) = civil(ms.div_euclid(1000).div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
#[path = "../tests/unit/line.rs"]
mod tests;
