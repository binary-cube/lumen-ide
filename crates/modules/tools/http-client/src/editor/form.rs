//! The editor's form: the fields being typed in, made from a [`Draft`] and back into one.

use http_requests::draft::{self, Auth, Body, Draft, Field, Part};
use ui_components::MultilineField;
use ui_kit::TextInput;

/// A row of a table (a parameter, a header, a form's field): its name, its value, on or off;
/// a multipart field may be a file.
#[derive(Debug)]
pub struct Row {
    pub name: TextInput,
    pub value: TextInput,
    pub enabled: bool,
    pub file: bool,
}

impl Row {
    pub fn new(name: &str, value: &str, enabled: bool) -> Self {
        Self { name: TextInput::new(name), value: TextInput::new(value), enabled, file: false }
    }

    fn field(&self) -> Field {
        Field {
            name: self.name.text().trim().to_owned(),
            value: self.value.text().to_owned(),
            enabled: self.enabled,
        }
    }

    fn is_empty(&self) -> bool {
        self.name.text().trim().is_empty() && self.value.text().trim().is_empty()
    }
}

/// The tables of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Params,
    Headers,
    Form,
    Multipart,
}

/// What its body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    None,
    FormData,
    UrlEncoded,
    GraphQl,
    Json,
    Xml,
    Yaml,
    Text,
    Other,
    File,
}

impl BodyKind {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::FormData,
        Self::UrlEncoded,
        Self::GraphQl,
        Self::Json,
        Self::Xml,
        Self::Yaml,
        Self::Text,
        Self::Other,
        Self::File,
    ];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::None => "http.body_none",
            Self::FormData => "http.body_form_data",
            Self::UrlEncoded => "http.body_urlencoded",
            Self::GraphQl => "http.body_graphql",
            Self::Json => "http.body_json",
            Self::Xml => "http.body_xml",
            Self::Yaml => "http.body_yaml",
            Self::Text => "http.body_text",
            Self::Other => "http.body_other",
            Self::File => "http.body_file",
        }
    }

    /// The editor's language for its text (its colors).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Json | Self::GraphQl => "json",
            Self::Xml => "xml",
            Self::Yaml => "yaml",
            _ => "txt",
        }
    }
}

/// How it authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    None,
    ApiKey,
    Basic,
    Digest,
    Bearer,
    OAuth2,
}

impl AuthKind {
    pub const ALL: [Self; 6] =
        [Self::None, Self::ApiKey, Self::Basic, Self::Digest, Self::Bearer, Self::OAuth2];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::None => "http.auth_none",
            Self::ApiKey => "http.auth_api_key",
            Self::Basic => "http.auth_basic",
            Self::Digest => "http.auth_digest",
            Self::Bearer => "http.auth_bearer",
            Self::OAuth2 => "http.auth_oauth2",
        }
    }

    /// Its fields' labels (their text keys).
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            Self::None => &[],
            Self::ApiKey => &["http.auth_key_name", "http.auth_key_value"],
            Self::Basic | Self::Digest => &["http.auth_user", "http.auth_password"],
            Self::Bearer => &["http.auth_token"],
            Self::OAuth2 => &["http.auth_config"],
        }
    }
}

/// The methods offered (another can be typed in their list's search).
pub const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// The form, as typed.
#[derive(Debug)]
pub struct Form {
    pub title: TextInput,
    pub name: Option<String>,
    /// What the editor doesn't show, written back as it was.
    pub kept: draft::Kept,
    pub method: String,
    pub url: TextInput,
    pub params: Vec<Row>,
    pub headers: Vec<Row>,
    pub body_kind: BodyKind,
    pub body: MultilineField,
    pub form: Vec<Row>,
    pub parts: Vec<Row>,
    pub variables: MultilineField,
    pub file: TextInput,
    pub auth_kind: AuthKind,
    pub auth: Vec<TextInput>,
    /// API Key: in a query parameter (else a header).
    pub key_in_query: bool,
    pub pre: MultilineField,
    pub post: MultilineField,
    pub docs: MultilineField,
    /// Its settings (the file's directives): redirects followed, cookies kept, logged.
    pub follow_redirects: bool,
    pub cookie_jar: bool,
    pub history: bool,
    /// What a URL can't hold percent-encoded (off: `@no-auto-encoding`).
    pub encoding: bool,
    /// `@timeout` and `@connection-timeout` as written (`5`, `500 ms`, `2 m`; empty: the
    /// default).
    pub timeout: TextInput,
    pub connect_timeout: TextInput,
    /// Its other directives, as they were.
    directives: Vec<String>,
}

/// The directives the settings tab shows (the rest kept as written).
fn setting(directive: &str) -> Option<(&str, &str)> {
    let (key, value) = directive
        .strip_prefix('@')?
        .split_once(char::is_whitespace)
        .unwrap_or((&directive[1..], ""));
    ["no-redirect", "no-cookie-jar", "no-log", "no-auto-encoding", "timeout", "connection-timeout"]
        .contains(&key)
        .then_some((key, value.trim()))
}

impl Form {
    pub fn of(draft: &Draft) -> Self {
        let rows = |fields: &[Field]| -> Vec<Row> {
            fields.iter().map(|f| Row::new(&f.name, &f.value, f.enabled)).collect()
        };
        let (body_kind, text, form, parts, variables, file) = match &draft.body {
            Body::None => (
                BodyKind::None,
                String::new(),
                Vec::new(),
                Vec::new(),
                String::new(),
                String::new(),
            ),
            Body::Json(t) => {
                (BodyKind::Json, t.clone(), Vec::new(), Vec::new(), String::new(), String::new())
            }
            Body::Xml(t) => {
                (BodyKind::Xml, t.clone(), Vec::new(), Vec::new(), String::new(), String::new())
            }
            Body::Yaml(t) => {
                (BodyKind::Yaml, t.clone(), Vec::new(), Vec::new(), String::new(), String::new())
            }
            Body::Text(t) => {
                (BodyKind::Text, t.clone(), Vec::new(), Vec::new(), String::new(), String::new())
            }
            Body::Other(t) => {
                (BodyKind::Other, t.clone(), Vec::new(), Vec::new(), String::new(), String::new())
            }
            Body::UrlEncoded(f) => (
                BodyKind::UrlEncoded,
                String::new(),
                rows(f),
                Vec::new(),
                String::new(),
                String::new(),
            ),
            Body::Multipart(p) => {
                let parts = p
                    .iter()
                    .map(|p| Row { file: p.file, ..Row::new(&p.name, &p.value, true) })
                    .collect();
                (BodyKind::FormData, String::new(), Vec::new(), parts, String::new(), String::new())
            }
            Body::GraphQl { query, variables } => (
                BodyKind::GraphQl,
                query.clone(),
                Vec::new(),
                Vec::new(),
                variables.clone(),
                String::new(),
            ),
            Body::File(path) => {
                (BodyKind::File, String::new(), Vec::new(), Vec::new(), String::new(), path.clone())
            }
        };
        let (auth_kind, auth, key_in_query): (AuthKind, Vec<&str>, bool) = match &draft.auth {
            Auth::None => (AuthKind::None, vec![], false),
            Auth::ApiKey { name, value, in_query } => {
                (AuthKind::ApiKey, vec![name, value], *in_query)
            }
            Auth::Basic { user, password } => (AuthKind::Basic, vec![user, password], false),
            Auth::Digest { user, password } => (AuthKind::Digest, vec![user, password], false),
            Auth::Bearer { token } => (AuthKind::Bearer, vec![token], false),
            Auth::OAuth2 { id } => (AuthKind::OAuth2, vec![id], false),
        };
        let mut form = Self {
            title: TextInput::new(&draft.title),
            name: draft.name.clone(),
            kept: draft.kept.clone(),
            method: if draft.method.eq_ignore_ascii_case("GRAPHQL") {
                "POST".into()
            } else {
                draft.method.clone()
            },
            url: TextInput::new(&draft::join_query(&draft.url, &draft.params)),
            params: rows(&draft.params),
            headers: rows(&draft.headers),
            body_kind,
            body: MultilineField::new(&text),
            form,
            parts,
            variables: MultilineField::new(&variables),
            file: TextInput::new(&file),
            auth_kind,
            auth: Vec::new(),
            key_in_query,
            pre: MultilineField::new(draft.pre_script.as_deref().unwrap_or_default()),
            post: MultilineField::new(draft.post_script.as_deref().unwrap_or_default()),
            docs: MultilineField::new(&draft.docs),
            follow_redirects: true,
            cookie_jar: true,
            history: true,
            encoding: true,
            timeout: TextInput::new(""),
            connect_timeout: TextInput::new(""),
            directives: Vec::new(),
        };
        for directive in &draft.directives {
            match setting(directive) {
                Some(("no-redirect", _)) => form.follow_redirects = false,
                Some(("no-cookie-jar", _)) => form.cookie_jar = false,
                Some(("no-log", _)) => form.history = false,
                Some(("no-auto-encoding", _)) => form.encoding = false,
                Some(("timeout", value)) => form.timeout.set_text(value),
                Some(("connection-timeout", value)) => form.connect_timeout.set_text(value),
                _ => form.directives.push(directive.clone()),
            }
        }
        form.set_auth_kind(auth_kind);
        for (input, value) in form.auth.iter_mut().zip(auth) {
            input.set_text(value);
        }
        form
    }

    /// The draft it makes.
    pub fn draft(&self) -> Draft {
        let fields = |rows: &[Row]| -> Vec<Field> {
            rows.iter().filter(|r| !r.is_empty()).map(Row::field).collect()
        };
        let text = |f: &MultilineField| f.text().to_owned();
        let body = match self.body_kind {
            BodyKind::None => Body::None,
            BodyKind::Json => Body::Json(text(&self.body)),
            BodyKind::Xml => Body::Xml(text(&self.body)),
            BodyKind::Yaml => Body::Yaml(text(&self.body)),
            BodyKind::Text => Body::Text(text(&self.body)),
            BodyKind::Other => Body::Other(text(&self.body)),
            BodyKind::UrlEncoded => Body::UrlEncoded(fields(&self.form)),
            BodyKind::FormData => Body::Multipart(
                self.parts
                    .iter()
                    .filter(|r| !r.is_empty() && r.enabled)
                    .map(|r| Part {
                        name: r.name.text().trim().to_owned(),
                        value: r.value.text().to_owned(),
                        file: r.file,
                    })
                    .collect(),
            ),
            BodyKind::GraphQl => {
                Body::GraphQl { query: text(&self.body), variables: text(&self.variables) }
            }
            BodyKind::File => Body::File(self.file.text().trim().to_owned()),
        };
        let value = |i: usize| self.auth.get(i).map(|f| f.text().to_owned()).unwrap_or_default();
        let auth = match self.auth_kind {
            AuthKind::None => Auth::None,
            AuthKind::ApiKey => {
                Auth::ApiKey { name: value(0), value: value(1), in_query: self.key_in_query }
            }
            AuthKind::Basic => Auth::Basic { user: value(0), password: value(1) },
            AuthKind::Digest => Auth::Digest { user: value(0), password: value(1) },
            AuthKind::Bearer => Auth::Bearer { token: value(0) },
            AuthKind::OAuth2 => Auth::OAuth2 { id: value(0) },
        };
        let (url, _) = draft::split_query(self.url.text().trim());
        let script =
            |f: &MultilineField| Some(f.text().to_owned()).filter(|t| !t.trim().is_empty());
        Draft {
            title: self.title.text().trim().to_owned(),
            name: self.name.clone(),
            docs: self.docs.text().trim_end().to_owned(),
            method: self.method.clone(),
            url,
            params: fields(&self.params),
            headers: fields(&self.headers),
            auth,
            body,
            pre_script: script(&self.pre),
            post_script: script(&self.post),
            directives: self.all_directives(),
            kept: self.kept.clone(),
        }
    }

    /// Its directives: the settings', then the rest as they were.
    fn all_directives(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.follow_redirects {
            out.push("@no-redirect".to_owned());
        }
        if !self.cookie_jar {
            out.push("@no-cookie-jar".to_owned());
        }
        if !self.history {
            out.push("@no-log".to_owned());
        }
        if !self.encoding {
            out.push("@no-auto-encoding".to_owned());
        }
        for (key, input) in
            [("timeout", &self.timeout), ("connection-timeout", &self.connect_timeout)]
        {
            let value = input.text().trim();
            if !value.is_empty() {
                out.push(format!("@{key} {value}"));
            }
        }
        out.extend(self.directives.iter().cloned());
        out
    }

    /// The table `table`'s rows.
    pub fn rows(&mut self, table: Table) -> &mut Vec<Row> {
        match table {
            Table::Params => &mut self.params,
            Table::Headers => &mut self.headers,
            Table::Form => &mut self.form,
            Table::Multipart => &mut self.parts,
        }
    }

    /// The URL typed: its query read into the parameters (the disabled ones kept).
    pub fn url_typed(&mut self) {
        let (_, params) = draft::split_query(self.url.text().trim());
        let disabled: Vec<Row> =
            std::mem::take(&mut self.params).into_iter().filter(|r| !r.enabled).collect();
        self.params = params.iter().map(|f| Row::new(&f.name, &f.value, true)).collect();
        self.params.extend(disabled);
    }

    /// A parameter changed: the URL's query written again from them.
    pub fn params_changed(&mut self) {
        let (base, _) = draft::split_query(self.url.text().trim());
        let fields: Vec<Field> =
            self.params.iter().filter(|r| !r.is_empty()).map(Row::field).collect();
        self.url.set_text(&draft::join_query(&base, &fields));
    }

    /// Its authentication's kind chosen (its fields made for it, what fits kept).
    pub fn set_auth_kind(&mut self, kind: AuthKind) {
        self.auth_kind = kind;
        let kept: Vec<String> = self.auth.iter().map(|f| f.text().to_owned()).collect();
        self.auth = kind
            .fields()
            .iter()
            .enumerate()
            .map(|(i, _)| TextInput::new(kept.get(i).map_or("", String::as_str)))
            .collect();
        if let Some(password) =
            self.auth.get_mut(1).filter(|_| matches!(kind, AuthKind::Basic | AuthKind::Digest))
        {
            password.set_masked(true);
        }
    }

    /// Its body's kind chosen (the text kept between text kinds).
    pub fn set_body_kind(&mut self, kind: BodyKind) {
        self.body_kind = kind;
    }

    /// The `Content-Type` its body's kind goes with: the draft's, without copying the
    /// body (asked at each frame of the Headers tab).
    fn body_content_type(&self) -> Option<&'static str> {
        let empty = String::new;
        match self.body_kind {
            BodyKind::None => Body::None,
            BodyKind::Json => Body::Json(empty()),
            BodyKind::Xml => Body::Xml(empty()),
            BodyKind::Yaml => Body::Yaml(empty()),
            BodyKind::Text => Body::Text(empty()),
            BodyKind::Other => Body::Other(empty()),
            BodyKind::UrlEncoded => Body::UrlEncoded(Vec::new()),
            BodyKind::FormData => Body::Multipart(Vec::new()),
            BodyKind::GraphQl => Body::GraphQl { query: empty(), variables: empty() },
            BodyKind::File => Body::File(empty()),
        }
        .content_type()
    }

    /// The headers the app adds itself (shown dimmed, not edited): `Content-Type` from the
    /// body, `Host`, `User-Agent`.
    pub fn automatic_headers(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let has = |name: &str| {
            self.headers
                .iter()
                .any(|r| r.enabled && r.name.text().trim().eq_ignore_ascii_case(name))
        };
        if let Some(kind) = self.body_content_type().filter(|_| !has("content-type")) {
            out.push(("Content-Type".to_owned(), kind.to_owned()));
        }
        if !has("host") {
            out.push(("Host".to_owned(), i18n::t("http.calculated")));
        }
        if !has("user-agent") {
            out.push(("User-Agent".to_owned(), kernel::brand::named("{app}")));
        }
        out
    }
}

#[cfg(test)]
#[path = "../../tests/unit/editor/form.rs"]
mod tests;
