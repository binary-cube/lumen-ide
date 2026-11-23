//! Interface translations.
//!
//! - **Catalogs** per language (`ro`, `en`…), in simple TOML files (`key = "message"`,
//!   with sections `[status]` → `status.key`). The core has its own catalogs; modules can
//!   contribute their own catalogs ([`register`]).
//! - **ICU messages:** `{name}`, `{count, plural, one {# eroare} few {# erori} other {# de
//!   erori}}`, `=0`, `{x, select, …}`. Plurals follow the language's CLDR rules.
//! - **The current language** is global ([`set_locale`]); without a translation in the current
//!   language, English is used, then the key itself.
//!
//! A message is parsed once, the first time it's asked for (most of a catalog's never are
//! in a session); its text is borrowed from the catalog built into the binary. [`t`] /
//! [`tf`] then only format.

mod catalog;
mod message;
mod plural;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, OnceLock, RwLock};

pub use catalog::parse_catalog;
pub use message::{Message, Value};
pub use plural::{Locale, PluralCategory};

/// The interface languages: (code, name in that language).
pub const LANGUAGES: &[(&str, &str)] = &[("ro", "Română"), ("en", "English")];

const FALLBACK: &str = "en";

#[derive(Default)]
struct Registry {
    locale: String,
    /// The messages, per language.
    catalogs: HashMap<String, HashMap<Box<str>, Entry>>,
}

/// A catalog's message: its text (the catalog's own, borrowed, unless it had escapes), parsed
/// when first asked for.
struct Entry {
    text: Cow<'static, str>,
    parsed: OnceLock<Arc<Message>>,
}

impl Entry {
    fn new(text: Cow<'static, str>) -> Self {
        Self { text, parsed: OnceLock::new() }
    }

    fn message(&self) -> Arc<Message> {
        Arc::clone(self.parsed.get_or_init(|| Arc::new(Message::parse(&self.text))))
    }
}

static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| {
    let mut registry = Registry { locale: "en".into(), ..Registry::default() };
    for (locale, source) in
        [("ro", include_str!("../locales/ro.toml")), ("en", include_str!("../locales/en.toml"))]
    {
        let catalog = registry.catalogs.entry(locale.to_owned()).or_default();
        for (key, text) in catalog::parse_catalog_static(source) {
            catalog.insert(key.into_boxed_str(), Entry::new(text));
        }
    }
    RwLock::new(registry)
});

/// Bumped whenever what a message says may change (a catalog added, the language changed):
/// a text made from messages is kept under it and made again only when it moves.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the messages say now, as a number that changes when they may (one atomic read).
pub fn generation() -> u64 {
    GENERATION.load(std::sync::atomic::Ordering::Acquire)
}

fn bump() {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Adds (or replaces) a catalog's messages, e.g. a module's contribution.
pub fn register(locale: &str, source: &str) {
    if let Ok(mut registry) = REGISTRY.write() {
        let catalog = registry.catalogs.entry(locale.to_owned()).or_default();
        for (key, text) in parse_catalog(source) {
            catalog.insert(key.into_boxed_str(), Entry::new(Cow::Owned(text)));
        }
    }
    bump();
}

/// Changes the interface language (e.g. `ro`, `en`).
pub fn set_locale(locale: &str) {
    if let Ok(mut registry) = REGISTRY.write() {
        locale.clone_into(&mut registry.locale);
    }
    bump();
}

pub fn locale() -> String {
    REGISTRY.read().map(|r| r.locale.clone()).unwrap_or_else(|_| FALLBACK.into())
}

fn lookup(key: &str) -> Option<(Arc<Message>, Locale)> {
    let registry = REGISTRY.read().ok()?;
    [registry.locale.as_str(), FALLBACK].into_iter().find_map(|locale| {
        let message = registry.catalogs.get(locale)?.get(key)?.message();
        Some((message, Locale::from_tag(locale)))
    })
}

/// A size in bytes, in the current language: `812 B`, `12 KB`, `3,4 MB`, `24 MB`,
/// `1,2 GB` (one decimal below 10, in that language's decimal separator).
pub fn bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if n < KB {
        return format!("{n} B");
    }
    if n < MB {
        return format!("{} KB", n.div_ceil(KB));
    }
    let (value, unit) = if n < MB * 1024 {
        (n as f64 / MB as f64, "MB")
    } else {
        (n as f64 / (MB * 1024) as f64, "GB")
    };
    let number = if value < 10.0 { format!("{value:.1}") } else { format!("{value:.0}") };
    let number = match Locale::from_tag(&locale()) {
        Locale::Ro => number.replace('.', ","),
        _ => number,
    };
    format!("{number} {unit}")
}

/// A number written with `.` (`3.4`) in the current language's decimal separator (`3,4` in
/// Romanian).
pub fn decimal(number: String) -> String {
    match Locale::from_tag(&locale()) {
        Locale::Ro => number.replace('.', ","),
        _ => number,
    }
}

/// How long ago, in the current language: "just now" under a minute, then minutes, hours,
/// days, months, years ("3 days ago").
pub fn ago(seconds: i64) -> String {
    let (key, n) = match seconds.max(0) {
        s if s < 60 => return t("time.just_now"),
        s if s < 3600 => ("time.ago_minutes", s / 60),
        s if s < 86_400 => ("time.ago_hours", s / 3600),
        s if s < 86_400 * 30 => ("time.ago_days", s / 86_400),
        s if s < 86_400 * 365 => ("time.ago_months", s / (86_400 * 30)),
        s => ("time.ago_years", s / (86_400 * 365)),
    };
    tf(key, &[("n", n.into())])
}

/// The message `key` in the current language (without arguments).
pub fn t(key: &str) -> String {
    tf(key, &[])
}

/// The message `key` in every language, one after another (the words a search finds
/// something by, whatever the interface's language); empty if no language has it.
pub fn in_every_language(key: &str) -> String {
    let Ok(registry) = REGISTRY.read() else { return String::new() };
    let found = LANGUAGES.iter().filter_map(|(code, _)| {
        let message = registry.catalogs.get(*code)?.get(key)?.message();
        Some(message.format(Locale::from_tag(code), &[]))
    });
    found.collect::<Vec<_>>().join(" ")
}

/// The message `key` in the current language, with arguments.
pub fn tf(key: &str, args: &[(&str, Value)]) -> String {
    match lookup(key) {
        Some((message, locale)) => message.format(locale, args),
        None => key.to_owned(),
    }
}

/// `tr!("status.errors", count = 3)` = `tf("status.errors", &[("count", 3.into())])`.
#[macro_export]
macro_rules! tr {
    ($key:expr) => { $crate::t($key) };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::tf($key, &[$((stringify!($name), $crate::Value::from($value))),+])
    };
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
