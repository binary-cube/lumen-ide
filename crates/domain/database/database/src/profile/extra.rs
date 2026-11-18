//! What an engine says that the model has no field of its own for: kept as [`Extra`], with
//! its type and unit, never dropped (a newer server's keys show without new code).

/// The unit of a number (its formatting is the UI's: `1.2 GB`, `845 ms`, `12 340 rows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Rows,
    Millis,
    Bytes,
    /// The engine's pages of `block_size` bytes (PostgreSQL's buffers).
    Blocks,
    Pages,
    Count,
    Percent,
    Cost,
    Ratio,
    None,
}

impl Unit {
    pub const ALL: [Unit; 10] = [
        Unit::Rows,
        Unit::Millis,
        Unit::Bytes,
        Unit::Blocks,
        Unit::Pages,
        Unit::Count,
        Unit::Percent,
        Unit::Cost,
        Unit::Ratio,
        Unit::None,
    ];

    /// Stable (the exported JSON's, the protocol's): `rows`, `ms`, `bytes`…
    pub fn id(self) -> &'static str {
        match self {
            Unit::Rows => "rows",
            Unit::Millis => "ms",
            Unit::Bytes => "bytes",
            Unit::Blocks => "blocks",
            Unit::Pages => "pages",
            Unit::Count => "count",
            Unit::Percent => "percent",
            Unit::Cost => "cost",
            Unit::Ratio => "ratio",
            Unit::None => "none",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.id() == id)
    }
}

/// A number with its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub unit: Unit,
}

impl Quantity {
    pub const fn new(value: f64, unit: Unit) -> Self {
        Self { value, unit }
    }
}

/// An extra's value: a JSON object the driver doesn't know becomes a [`ExtraValue::Group`],
/// a list a [`ExtraValue::List`] — nothing is thrown away.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtraValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Quantity(Quantity),
    List(Vec<ExtraValue>),
    Group(Vec<Extra>),
}

impl ExtraValue {
    /// Stable (the exported JSON's `type`).
    pub fn type_id(&self) -> &'static str {
        match self {
            ExtraValue::Bool(_) => "bool",
            ExtraValue::Int(_) => "int",
            ExtraValue::Float(_) => "float",
            ExtraValue::Text(_) => "text",
            ExtraValue::Quantity(_) => "quantity",
            ExtraValue::List(_) => "list",
            ExtraValue::Group(_) => "group",
        }
    }

    /// As a number, when it is one.
    pub fn number(&self) -> Option<f64> {
        match self {
            ExtraValue::Int(n) => Some(*n as f64),
            ExtraValue::Float(n) => Some(*n),
            ExtraValue::Quantity(q) => Some(q.value),
            _ => None,
        }
    }

    /// Its text, as written in the plain export (a group as `{a: 1, b: 2}`).
    pub fn text(&self) -> String {
        match self {
            ExtraValue::Bool(b) => b.to_string(),
            ExtraValue::Int(n) => n.to_string(),
            ExtraValue::Float(n) => crate::profile::export::number(*n),
            ExtraValue::Text(t) => t.clone(),
            ExtraValue::Quantity(q) => match q.unit {
                Unit::None | Unit::Count | Unit::Ratio | Unit::Cost => {
                    crate::profile::export::number(q.value)
                }
                unit => format!("{} {}", crate::profile::export::number(q.value), unit.id()),
            },
            ExtraValue::List(items) => {
                let parts: Vec<String> = items.iter().map(ExtraValue::text).collect();
                format!("[{}]", parts.join(", "))
            }
            ExtraValue::Group(items) => {
                let parts: Vec<String> =
                    items.iter().map(|e| format!("{}: {}", e.key, e.value.text())).collect();
                format!("{{{}}}", parts.join(", "))
            }
        }
    }
}

/// Something an engine said beyond the model's fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    /// The engine's own key (`Sort Method`, `r_filtered`, `Async Capable`).
    pub key: String,
    /// Its label, when the driver gives one (the UI's comes from the locales by key).
    pub label: Option<String>,
    /// The section it's shown in (`JIT`, `Triggers`, `Workers`; none: the others).
    pub group: Option<String>,
    pub value: ExtraValue,
}

impl Extra {
    pub fn new(key: impl Into<String>, value: ExtraValue) -> Self {
        Self { key: key.into(), label: None, group: None, value }
    }

    pub fn in_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// The extra of `key` in `list`.
pub fn find<'a>(list: &'a [Extra], key: &str) -> Option<&'a Extra> {
    list.iter().find(|e| e.key == key)
}
