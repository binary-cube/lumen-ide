//! A cell's style as draw.io writes it (`rounded=1;whiteSpace=wrap;fillColor=#dae8fc;`): its
//! entries in their order — `key=value`, or a bare name (`ellipse;`, a style of the
//! stylesheet) — and whether it ends with `;`. Written back the same; a key changed stays in
//! its place, a new one goes at the end.

/// A style: its entries' text kept as one string (`key=value;name;…`, without its last `;`)
/// and where each entry is in it — two allocations a style, not two an entry (a page holds
/// thousands, and every edit's history keeps copies).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    /// The entries, joined by `;`.
    text: String,
    /// Each entry's place in `text`.
    entries: Vec<Entry>,
    /// It ended with `;` (draw.io's do).
    trailing: bool,
}

/// An entry's place: its key from `start` to `key_end`; its value, when it has one (a `=`
/// at `key_end`), from `key_end + 1` to `end`. A bare name: `key_end == end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    start: u32,
    key_end: u32,
    end: u32,
}

/// A place in a style's text (a file is at most a few tens of megabytes: it fits).
fn at(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Style {
    /// `text` read (an empty entry, `;;`, is dropped).
    pub fn parse(text: &str) -> Self {
        let mut style = Self::from_entries(text.split(';').filter(|e| !e.is_empty()).map(|e| {
            match e.split_once('=') {
                Some((k, v)) => (k, Some(v)),
                None => (e, None),
            }
        }));
        style.trailing = text.ends_with(';');
        style
    }

    /// A style of `entries`, in order (no trailing `;`).
    fn from_entries<'a>(entries: impl Iterator<Item = (&'a str, Option<&'a str>)>) -> Self {
        let mut style = Self::default();
        for (k, v) in entries {
            if !style.text.is_empty() {
                style.text.push(';');
            }
            let start = at(style.text.len());
            style.text.push_str(k);
            let key_end = at(style.text.len());
            if let Some(v) = v {
                style.text.push('=');
                style.text.push_str(v);
            }
            style.entries.push(Entry { start, key_end, end: at(style.text.len()) });
        }
        style.text.shrink_to_fit();
        style.entries.shrink_to_fit();
        style
    }

    /// An entry's key and value.
    fn entry(&self, e: Entry) -> (&str, Option<&str>) {
        let (start, key_end, end) = (e.start as usize, e.key_end as usize, e.end as usize);
        let key = self.text.get(start..key_end).unwrap_or_default();
        let value = (key_end < end).then(|| self.text.get(key_end + 1..end).unwrap_or_default());
        (key, value)
    }

    /// The same with `edit` applied to its entries (in order), its ending kept.
    fn rebuilt<'a>(&'a self, edit: impl FnOnce(&mut Vec<(&'a str, Option<&'a str>)>)) -> Self {
        let mut entries: Vec<(&'a str, Option<&'a str>)> = self.entries().collect();
        edit(&mut entries);
        let mut style = Self::from_entries(entries.into_iter());
        style.trailing = self.trailing;
        style
    }

    /// The value of `key` (`None`: not set; a bare name has no value).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries().find(|(k, _)| *k == key).and_then(|(_, v)| v)
    }

    /// `key` set to `value`, in its place or at the end.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        let first = self.entries.is_empty();
        *self = self.rebuilt(|entries| match entries.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = Some(&value),
            None => entries.push((key, Some(&value))),
        });
        // A style made here ends as draw.io's do.
        if first {
            self.trailing = true;
        }
    }

    /// `key` taken away.
    pub fn remove(&mut self, key: &str) {
        if self.get(key).is_none() && !self.entries().any(|(k, _)| k == key) {
            return;
        }
        *self = self.rebuilt(|entries| entries.retain(|(k, _)| *k != key));
    }

    /// Whether the style names `name` bare (`ellipse;`, `text;`, `group;`).
    pub fn has_name(&self, name: &str) -> bool {
        self.entries().any(|(k, v)| v.is_none() && k == name)
    }

    /// What the cell is drawn as: its `shape`, else its first bare name (`ellipse`,
    /// `rhombus`…); `None`: draw.io's default (a rectangle, a connector for an edge).
    pub fn shape(&self) -> Option<&str> {
        self.get("shape").or_else(|| self.entries().find(|(_, v)| v.is_none()).map(|(k, _)| k))
    }

    /// `key` as a flag (`1`: on).
    pub fn flag(&self, key: &str) -> bool {
        self.get(key) == Some("1")
    }

    /// `key` as a number.
    pub fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.trim().parse().ok()).filter(|v: &f64| v.is_finite())
    }

    /// The entries, in order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.entries.iter().map(|e| self.entry(*e))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether it's what `text` reads as (its entries, its ending), without reading it.
    pub(crate) fn reads_as(&self, text: &str) -> bool {
        let theirs = text.split(';').filter(|e| !e.is_empty()).map(|e| match e.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (e, None),
        });
        self.entries().eq(theirs) && self.trailing == text.ends_with(';')
    }

    /// What it holds in memory, roughly.
    pub(crate) fn weight(&self) -> usize {
        self.text.capacity() + self.entries.capacity() * std::mem::size_of::<Entry>()
    }
}

impl std::fmt::Display for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)?;
        if self.trailing && !self.entries.is_empty() {
            f.write_str(";")?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/style.rs"]
mod tests;
