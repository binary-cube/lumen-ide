//! A driver's connection settings, described as data ([`ConnectionSpec`]: its fields, each
//! a [`FieldSpec`], and how its URL is written): the data sources' modal draws them,
//! checks them and keeps the URL in step with them both ways, without knowing the driver —
//! so a third party's driver (37i) describes its own the same way, with no code here.
//!
//! A field is bound to a part of a [`DataSource`] (its host, port, user, database, file, or
//! one of its target's options); the password is a field too, but never a part of the
//! source: it goes where the source's [`crate::SecretPolicy`] says. Everything is pure.

use std::path::PathBuf;

use crate::DataSource;
use kernel::Secret;

/// What kind of value a field holds (how it's edited and checked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    /// A whole number between `min` and `max` (a port, seconds).
    Number {
        min: i64,
        max: i64,
    },
    /// A secret (the password): masked, never kept with the source.
    Secret,
    /// A file on disk; `extensions` (lowercase, without the dot) are those offered first.
    File {
        extensions: Vec<String>,
    },
    /// One of these values, each with its label (an i18n key, or the text itself).
    Choice(Vec<(String, String)>),
    /// On or off (`true` / `false`).
    Flag,
}

/// Where a field is shown in the data sources' modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldTab {
    General,
    Options,
    Ssl,
    Ssh,
}

/// The part of a source a field edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Host,
    Port,
    User,
    Database,
    File,
    /// The secret: never in the source (see the module's doc).
    Password,
    /// One of its target's options (`sslmode`, `connect_timeout`…).
    Option(String),
}

/// One of a driver's connection fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    /// Stable within its driver (`host`, `sslmode`).
    pub id: String,
    /// Its label: an i18n key (`datasources.field_host`) or, for a driver that brings no
    /// translations, the text itself.
    pub label: String,
    pub kind: FieldKind,
    pub binding: Binding,
    /// The value meant when none is given (shown as the field's hint; a choice's default
    /// option).
    pub default: String,
    pub required: bool,
    pub tab: FieldTab,
}

impl FieldSpec {
    pub fn new(id: &str, label: &str, kind: FieldKind, binding: Binding, tab: FieldTab) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            kind,
            binding,
            default: String::new(),
            required: false,
            tab,
        }
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = default.into();
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn is_secret(&self) -> bool {
        self.kind == FieldKind::Secret || self.binding == Binding::Password
    }
}

/// How a driver's URL is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlStyle {
    /// `scheme://user@host:port/database?option=value&…` (a server).
    Server,
    /// `scheme:path` (a file).
    File,
}

/// A driver's URL: its schemes (the first is the one written; `jdbc:` before any is
/// understood too) and its style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlSpec {
    pub schemes: Vec<String>,
    pub style: UrlStyle,
}

/// A driver's connection settings (see the module's doc).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConnectionSpec {
    pub fields: Vec<FieldSpec>,
    pub url: Option<UrlSpec>,
}

impl ConnectionSpec {
    /// A server's: host, port (`default_port`; 0: none known), user, password and
    /// database, its URL with `schemes` (none: no URL).
    pub fn server(default_port: u16, schemes: &[&str]) -> Self {
        use FieldTab::General;
        Self {
            fields: vec![
                FieldSpec::new(
                    "host",
                    "datasources.field_host",
                    FieldKind::Text,
                    Binding::Host,
                    General,
                )
                .with_default("localhost"),
                FieldSpec::new(
                    "port",
                    "datasources.field_port",
                    FieldKind::Number { min: 1, max: 65535 },
                    Binding::Port,
                    General,
                )
                .with_default(if default_port == 0 {
                    String::new()
                } else {
                    default_port.to_string()
                }),
                FieldSpec::new(
                    "user",
                    "datasources.field_user",
                    FieldKind::Text,
                    Binding::User,
                    General,
                ),
                FieldSpec::new(
                    "password",
                    "datasources.field_password",
                    FieldKind::Secret,
                    Binding::Password,
                    General,
                ),
                FieldSpec::new(
                    "database",
                    "datasources.field_database",
                    FieldKind::Text,
                    Binding::Database,
                    General,
                ),
            ],
            url: (!schemes.is_empty()).then(|| UrlSpec {
                schemes: schemes.iter().map(|s| (*s).to_owned()).collect(),
                style: UrlStyle::Server,
            }),
        }
    }

    /// A file's: the file (offering `extensions`), its URL `scheme:path`.
    pub fn file(extensions: &[&str], scheme: &str) -> Self {
        Self {
            fields: vec![
                FieldSpec::new(
                    "file",
                    "datasources.field_file",
                    FieldKind::File {
                        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
                    },
                    Binding::File,
                    FieldTab::General,
                )
                .required(),
            ],
            url: Some(UrlSpec { schemes: vec![scheme.to_owned()], style: UrlStyle::File }),
        }
    }

    /// `field` added last.
    pub fn with(mut self, field: FieldSpec) -> Self {
        self.fields.push(field);
        self
    }

    pub fn field(&self, id: &str) -> Option<&FieldSpec> {
        self.fields.iter().find(|f| f.id == id)
    }

    /// The fields shown in `tab`, in order.
    pub fn fields_in(&self, tab: FieldTab) -> impl Iterator<Item = &FieldSpec> {
        self.fields.iter().filter(move |f| f.tab == tab)
    }

    /// The options its fields edit (the others are the source's own, "advanced" ones).
    pub fn bound_options(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter_map(|f| match &f.binding {
                Binding::Option(name) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// A password is asked for (a server's; a file needs none).
    pub fn has_secret(&self) -> bool {
        self.fields.iter().any(FieldSpec::is_secret)
    }

    /// What's wrong with `source`'s values: each field's id and why.
    pub fn problems(&self, source: &DataSource) -> Vec<(String, FieldError)> {
        self.fields
            .iter()
            .filter(|f| !f.is_secret())
            .filter_map(|f| validate(f, &value(source, f)).err().map(|e| (f.id.clone(), e)))
            .collect()
    }
}

/// `field`'s value in `source`, as text (none: empty; the password: always empty).
pub fn value(source: &DataSource, field: &FieldSpec) -> String {
    let target = &source.target;
    match &field.binding {
        Binding::Host => target.host.clone(),
        Binding::Port => target.port.map(|p| p.to_string()).unwrap_or_default(),
        Binding::User => target.user.clone(),
        Binding::Database => target.database.clone(),
        Binding::File => {
            source.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
        }
        Binding::Password => String::new(),
        Binding::Option(name) => target.options.get(name).cloned().unwrap_or_default(),
    }
}

/// `field` of `source` set to `text` (trimmed; empty: none). A port that isn't one is left
/// out ([`validate`] says why); the password is never set here, nor an option named like
/// a secret.
pub fn set_value(source: &mut DataSource, field: &FieldSpec, text: &str) {
    let text = if field.is_secret() { "" } else { text.trim() };
    let target = &mut source.target;
    match &field.binding {
        Binding::Host => target.host = text.to_owned(),
        Binding::Port => target.port = text.parse().ok(),
        Binding::User => target.user = text.to_owned(),
        Binding::Database => target.database = text.to_owned(),
        Binding::File => source.file = (!text.is_empty()).then(|| PathBuf::from(text)),
        Binding::Password => {}
        Binding::Option(name) => {
            if text.is_empty() || crate::source::is_secret_option(name) {
                target.options.remove(name);
            } else {
                target.options.insert(name.clone(), text.to_owned());
            }
        }
    }
}

/// Why a field's value isn't accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldError {
    Required,
    NotANumber,
    OutOfRange {
        min: i64,
        max: i64,
    },
    /// Not one of the choices.
    NotAChoice,
    /// A switch that isn't `true` or `false`.
    NotAFlag,
}

/// Whether `text` is a value `field` takes (empty: only when it isn't required).
pub fn validate(field: &FieldSpec, text: &str) -> Result<(), FieldError> {
    let text = text.trim();
    if text.is_empty() {
        return if field.required { Err(FieldError::Required) } else { Ok(()) };
    }
    match &field.kind {
        FieldKind::Number { min, max } => {
            let n: i64 = text.parse().map_err(|_| FieldError::NotANumber)?;
            if n < *min || n > *max {
                return Err(FieldError::OutOfRange { min: *min, max: *max });
            }
            Ok(())
        }
        FieldKind::Choice(values) if !values.iter().any(|(v, _)| v == text) => {
            Err(FieldError::NotAChoice)
        }
        FieldKind::Flag if !matches!(text, "true" | "false") => Err(FieldError::NotAFlag),
        _ => Ok(()),
    }
}

/// Why a URL isn't understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    /// The driver has no URL.
    NoUrl,
    /// Not one of its schemes (`expected`: the one it writes).
    Scheme { expected: String },
    /// Something in it can't be read (what).
    Malformed(String),
    /// Its port isn't one.
    Port,
}

/// Percent-encodes what isn't plain in a URL's part (`keep`: what's left as it is there).
fn encode(text: &str, keep: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || "-._~".contains(c) || keep.contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A URL's part with its `%XX` decoded (`plus`: `+` is a space, as in a query).
fn decode(text: &str, plus: bool) -> Result<String, UrlError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = text.get(i + 1..i + 3).ok_or_else(|| UrlError::Malformed(text.into()))?;
                out.push(
                    u8::from_str_radix(hex, 16).map_err(|_| UrlError::Malformed(text.into()))?,
                );
                i += 3;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| UrlError::Malformed(text.into()))
}

/// `source`'s URL by `spec` (never with its password); `None` if the driver has none.
pub fn to_url(spec: &ConnectionSpec, source: &DataSource) -> Option<String> {
    let url = spec.url.as_ref()?;
    let scheme = url.schemes.first()?;
    let target = &source.target;
    Some(match url.style {
        UrlStyle::File => {
            let file = source.file.as_ref().map(|f| f.to_string_lossy().into_owned());
            format!("{scheme}:{}", file.unwrap_or_default())
        }
        UrlStyle::Server => {
            let mut out = format!("{scheme}://");
            if !target.user.is_empty() {
                out.push_str(&encode(&target.user, ""));
                out.push('@');
            }
            // An IPv6 address between brackets.
            if target.host.contains(':') {
                out.push_str(&format!("[{}]", target.host));
            } else {
                out.push_str(&encode(&target.host, ""));
            }
            if let Some(port) = target.port {
                out.push_str(&format!(":{port}"));
            }
            if !target.database.is_empty() {
                out.push('/');
                out.push_str(&encode(&target.database, ""));
            }
            let options: Vec<String> = target
                .options
                .iter()
                .filter(|(k, _)| !crate::source::is_secret_option(k))
                .filter(|(k, _)| {
                    ![crate::Target::READ_ONLY, crate::Target::STARTUP].contains(&k.as_str())
                })
                .map(|(k, v)| format!("{}={}", encode(k, ""), encode(v, "/:")))
                .collect();
            if !options.is_empty() {
                out.push('?');
                out.push_str(&options.join("&"));
            }
            out
        }
    })
}

/// `url` read into `source` by `spec`: its host, port, user, database (or file) and options
/// replace those it had (the options its fields don't edit are kept; those the URL names
/// are set). A password in it (`user:password@`) is given back, never kept in the source.
pub fn from_url(
    spec: &ConnectionSpec,
    url: &str,
    source: &mut DataSource,
) -> Result<Option<Secret>, UrlError> {
    let shape = spec.url.as_ref().ok_or(UrlError::NoUrl)?;
    let text = url.trim();
    let text = text.strip_prefix("jdbc:").unwrap_or(text);
    let expected = shape.schemes.first().cloned().unwrap_or_default();
    let (scheme, rest) =
        text.split_once(':').ok_or_else(|| UrlError::Scheme { expected: expected.clone() })?;
    if !shape.schemes.iter().any(|s| s.eq_ignore_ascii_case(scheme)) {
        return Err(UrlError::Scheme { expected });
    }
    match shape.style {
        UrlStyle::File => {
            let path = rest.strip_prefix("//").unwrap_or(rest);
            source.file = (!path.is_empty()).then(|| PathBuf::from(path));
            Ok(None)
        }
        UrlStyle::Server => {
            let rest = rest.strip_prefix("//").ok_or_else(|| UrlError::Malformed(text.into()))?;
            let (main, query) = rest.split_once('?').unwrap_or((rest, ""));
            let (authority, database) = main.split_once('/').unwrap_or((main, ""));
            let (credentials, host_port) = match authority.rsplit_once('@') {
                Some((c, h)) => (Some(c), h),
                None => (None, authority),
            };
            let (user, password) = match credentials {
                Some(c) => match c.split_once(':') {
                    Some((u, p)) => (decode(u, false)?, Some(decode(p, false)?)),
                    None => (decode(c, false)?, None),
                },
                None => (String::new(), None),
            };
            let (host, port) = if let Some(inner) = host_port.strip_prefix('[') {
                let (h, after) =
                    inner.split_once(']').ok_or_else(|| UrlError::Malformed(text.into()))?;
                (h.to_owned(), after.strip_prefix(':'))
            } else {
                match host_port.rsplit_once(':') {
                    Some((h, p)) => (decode(h, false)?, Some(p)),
                    None => (decode(host_port, false)?, None),
                }
            };
            let port = match port.filter(|p| !p.is_empty()) {
                Some(p) => Some(p.parse::<u16>().map_err(|_| UrlError::Port)?),
                None => None,
            };
            let mut options = Vec::new();
            for pair in query.split('&').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                options.push((decode(k, true)?, decode(v, true)?));
            }
            let target = &mut source.target;
            target.host = host;
            target.port = port;
            target.user = user;
            target.database = decode(database, false)?;
            // The options the URL can name: those of its fields are its own now.
            for name in spec.bound_options() {
                target.options.remove(name);
            }
            let mut secret = password.map(Secret::from);
            for (k, v) in options {
                // A password given as an option (`?password=…`) is a password too.
                if crate::source::is_secret_option(&k) {
                    if k.eq_ignore_ascii_case("password") && secret.is_none() {
                        secret = Some(Secret::from(v));
                    }
                    continue;
                }
                target.options.insert(k, v);
            }
            Ok(secret)
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/fields.rs"]
mod tests;
