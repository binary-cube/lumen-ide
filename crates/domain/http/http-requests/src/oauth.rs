//! OAuth 2.0 configured in an environment (plan 77j): `Security.Auth.<id>` in an environment
//! (`Type`, `Grant Type`, `Auth URL`, `Token URL`, `Redirect URL`, `Client ID`, `Client
//! Secret`, `Client Credentials`, `Scope`, `Username`, `Password`, `PKCE`, `Device Auth URL`,
//! `Use ID Token`, `Acquire Automatically`, `Custom Request Parameters`), used by a request as
//! `{{$auth.token("id")}}` / `{{$auth.idToken("id")}}`. Pure: what to send and how to read
//! the answers; the module sends and waits.

use serde_json::Value;

/// How a token is had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    AuthorizationCode,
    ClientCredentials,
    Password,
    DeviceAuthorization,
    Implicit,
}

/// How the client says who it is to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// `Authorization: Basic` (the default).
    Basic,
    /// `client_id` / `client_secret` in the body.
    InBody,
    /// Only `client_id`.
    None,
}

/// A configuration (its values' `{{variables}}` already put in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    /// `Mock`: these tokens, as they are.
    pub mock: Option<(String, Option<String>)>,
    pub grant: Grant,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    pub device_auth_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub client_auth: ClientAuth,
    pub scope: String,
    pub username: String,
    pub password: String,
    pub pkce: bool,
    pub use_id_token: bool,
    pub acquire_automatically: bool,
    /// Extra parameters (name, value, where: `None` everywhere, `Some(true)` the auth
    /// request only, `Some(false)` the token request only).
    pub custom: Vec<(String, String, Option<bool>)>,
}

/// `id`'s configuration in an environment's `Security` (`text` puts its values' variables
/// in). `None`: no such one.
pub fn config(security: &Value, id: &str, text: &dyn Fn(&str) -> String) -> Option<AuthConfig> {
    let auth = security.get("Auth")?.get(id)?;
    let field = |name: &str| auth.get(name).and_then(Value::as_str).map(text).unwrap_or_default();
    let flag =
        |name: &str, default: bool| auth.get(name).and_then(Value::as_bool).unwrap_or(default);
    if auth.get("Type").and_then(Value::as_str) == Some("Mock") {
        let id_token = Some(field("ID Token")).filter(|t| !t.is_empty());
        return Some(AuthConfig { mock: Some((field("Token"), id_token)), ..empty() });
    }
    let grant = match auth.get("Grant Type").and_then(Value::as_str).unwrap_or("Authorization Code")
    {
        "Client Credentials" => Grant::ClientCredentials,
        "Password" => Grant::Password,
        "Device Authorization" => Grant::DeviceAuthorization,
        "Implicit" => Grant::Implicit,
        _ => Grant::AuthorizationCode,
    };
    let client_auth = match auth.get("Client Credentials").and_then(Value::as_str) {
        Some("in body") => ClientAuth::InBody,
        Some("none") => ClientAuth::None,
        _ => ClientAuth::Basic,
    };
    let pkce = match auth.get("PKCE") {
        Some(Value::Bool(on)) => *on,
        Some(Value::Object(_)) => true,
        _ => false,
    };
    let mut custom = Vec::new();
    if let Some(Value::Object(params)) = auth.get("Custom Request Parameters") {
        for (name, value) in params {
            let (value, place) = match value {
                Value::Object(o) => (
                    o.get("Value").and_then(Value::as_str).map(text).unwrap_or_default(),
                    match o.get("Use").and_then(Value::as_str) {
                        Some("In Auth Request") => Some(true),
                        Some("In Token Request") => Some(false),
                        _ => None,
                    },
                ),
                Value::String(s) => (text(s), None),
                other => (other.to_string(), None),
            };
            custom.push((name.clone(), value, place));
        }
    }
    Some(AuthConfig {
        mock: None,
        grant,
        auth_url: field("Auth URL"),
        token_url: field("Token URL"),
        redirect_url: field("Redirect URL"),
        device_auth_url: field("Device Auth URL"),
        client_id: field("Client ID"),
        client_secret: field("Client Secret"),
        client_auth,
        scope: field("Scope"),
        username: field("Username"),
        password: field("Password"),
        pkce,
        use_id_token: flag("Use ID Token", false),
        acquire_automatically: flag("Acquire Automatically", true),
        custom,
    })
}

fn empty() -> AuthConfig {
    AuthConfig {
        mock: None,
        grant: Grant::ClientCredentials,
        auth_url: String::new(),
        token_url: String::new(),
        redirect_url: String::new(),
        device_auth_url: String::new(),
        client_id: String::new(),
        client_secret: String::new(),
        client_auth: ClientAuth::Basic,
        scope: String::new(),
        username: String::new(),
        password: String::new(),
        pkce: false,
        use_id_token: false,
        acquire_automatically: true,
        custom: Vec::new(),
    }
}

/// A request to an OAuth endpoint: POST of a form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPost {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub form: Vec<(String, String)>,
}

impl FormPost {
    /// The form's body (`application/x-www-form-urlencoded`).
    pub fn body(&self) -> String {
        self.form
            .iter()
            .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    }
}

/// `text` as a form's field encodes it.
pub fn form_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl AuthConfig {
    /// A token request with `fields` (the grant's), the client said as configured, the custom
    /// parameters for the token request in.
    pub fn token_request(&self, mut fields: Vec<(String, String)>) -> FormPost {
        let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
        match self.client_auth {
            ClientAuth::Basic if !self.client_secret.is_empty() => {
                let pair = format!(
                    "{}:{}",
                    form_encode(&self.client_id),
                    form_encode(&self.client_secret)
                );
                headers.push((
                    "Authorization".to_owned(),
                    format!("Basic {}", crate::curl::base64(pair.as_bytes())),
                ));
            }
            ClientAuth::InBody => {
                fields.push(("client_id".to_owned(), self.client_id.clone()));
                if !self.client_secret.is_empty() {
                    fields.push(("client_secret".to_owned(), self.client_secret.clone()));
                }
            }
            _ => fields.push(("client_id".to_owned(), self.client_id.clone())),
        }
        for (name, value, place) in &self.custom {
            if *place != Some(true) {
                fields.push((name.clone(), value.clone()));
            }
        }
        FormPost { url: self.token_url.clone(), headers, form: fields }
    }

    fn scope_field(&self) -> Vec<(String, String)> {
        if self.scope.is_empty() {
            Vec::new()
        } else {
            vec![("scope".to_owned(), self.scope.clone())]
        }
    }

    /// Client Credentials' token request.
    pub fn client_credentials(&self) -> FormPost {
        let mut fields = vec![("grant_type".to_owned(), "client_credentials".to_owned())];
        fields.extend(self.scope_field());
        self.token_request(fields)
    }

    /// Password's token request.
    pub fn password_grant(&self) -> FormPost {
        let mut fields = vec![
            ("grant_type".to_owned(), "password".to_owned()),
            ("username".to_owned(), self.username.clone()),
            ("password".to_owned(), self.password.clone()),
        ];
        fields.extend(self.scope_field());
        self.token_request(fields)
    }

    /// A refresh's token request.
    pub fn refresh(&self, refresh_token: &str) -> FormPost {
        self.token_request(vec![
            ("grant_type".to_owned(), "refresh_token".to_owned()),
            ("refresh_token".to_owned(), refresh_token.to_owned()),
        ])
    }

    /// The page the user signs in on (Authorization Code), with `state` and the PKCE
    /// challenge of `verifier` (when it's on).
    pub fn authorize_url(&self, redirect: &str, state: &str, verifier: &str) -> String {
        let mut params = vec![
            (
                "response_type".to_owned(),
                if self.grant == Grant::Implicit { "token" } else { "code" }.to_owned(),
            ),
            ("client_id".to_owned(), self.client_id.clone()),
            ("redirect_uri".to_owned(), redirect.to_owned()),
            ("state".to_owned(), state.to_owned()),
        ];
        params.extend(self.scope_field());
        // PKCE is the code's: an Implicit sign-in has none to exchange.
        if self.pkce && self.grant != Grant::Implicit {
            params.push(("code_challenge".to_owned(), challenge(verifier)));
            params.push(("code_challenge_method".to_owned(), "S256".to_owned()));
        }
        for (name, value, place) in &self.custom {
            if *place != Some(false) {
                params.push((name.clone(), value.clone()));
            }
        }
        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let join = if self.auth_url.contains('?') { '&' } else { '?' };
        format!("{}{join}{query}", self.auth_url)
    }

    /// The code the sign-in gave, exchanged for a token.
    pub fn exchange(&self, code: &str, redirect: &str, verifier: &str) -> FormPost {
        let mut fields = vec![
            ("grant_type".to_owned(), "authorization_code".to_owned()),
            ("code".to_owned(), code.to_owned()),
            ("redirect_uri".to_owned(), redirect.to_owned()),
        ];
        if self.pkce {
            fields.push(("code_verifier".to_owned(), verifier.to_owned()));
        }
        self.token_request(fields)
    }

    /// Device Authorization's first request (its device and user codes).
    pub fn device_request(&self) -> FormPost {
        let mut fields = vec![("client_id".to_owned(), self.client_id.clone())];
        fields.extend(self.scope_field());
        FormPost {
            url: self.device_auth_url.clone(),
            headers: vec![("Accept".to_owned(), "application/json".to_owned())],
            form: fields,
        }
    }

    /// Device Authorization's polling of the token endpoint.
    pub fn device_poll(&self, device_code: &str) -> FormPost {
        self.token_request(vec![
            ("grant_type".to_owned(), "urn:ietf:params:oauth:grant-type:device_code".to_owned()),
            ("device_code".to_owned(), device_code.to_owned()),
        ])
    }
}

/// PKCE's S256 challenge of `verifier`.
pub fn challenge(verifier: &str) -> String {
    crate::hash::base64(
        &crate::hash::digest(crate::hash::Algorithm::Sha256, verifier.as_bytes()),
        true,
    )
}

/// A token as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub access: String,
    pub id: Option<String>,
    pub refresh: Option<String>,
    /// When it ends (seconds since the epoch; `None`: it doesn't say).
    pub expires: Option<u64>,
}

impl Token {
    /// Still good at `now` (a minute's margin).
    pub fn fresh(&self, now: u64) -> bool {
        self.expires.is_none_or(|end| end > now.saturating_add(60))
    }
}

/// A token endpoint's answer read at `now` (`Err`: its error said).
pub fn read_token(body: &str, now: u64) -> Result<Token, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| format!("not a token: {}", body.chars().take(200).collect::<String>()))?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        let detail = value.get("error_description").and_then(Value::as_str).unwrap_or_default();
        return Err(format!("{error} {detail}").trim().to_owned());
    }
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(Token {
        access: text("access_token").ok_or_else(|| "no access_token in the answer".to_owned())?,
        id: text("id_token"),
        refresh: text("refresh_token"),
        expires: value.get("expires_in").and_then(Value::as_u64).map(|s| now.saturating_add(s)),
    })
}

/// A Device Authorization's first answer: the device's code, the user's, where the user
/// goes (with the code in it, when given), how often to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub fn read_device(body: &str) -> Result<Device, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(Device {
        device_code: text("device_code").ok_or("no device_code")?,
        user_code: text("user_code").unwrap_or_default(),
        verification_uri: text("verification_uri_complete")
            .or_else(|| text("verification_uri"))
            .or_else(|| text("verification_url"))
            .unwrap_or_default(),
        interval: value.get("interval").and_then(Value::as_u64).unwrap_or(5),
        expires_in: value.get("expires_in").and_then(Value::as_u64).unwrap_or(600),
    })
}

/// The `$auth.token("id")` / `$auth.idToken("id")` a text uses: (id, id token).
pub fn uses(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for (_, name) in crate::vars::references(text) {
        for (prefix, id_token) in [("$auth.token(", false), ("$auth.idToken(", true)] {
            if let Some(rest) = name.strip_prefix(prefix) {
                let id = rest.trim_end_matches(')').trim().trim_matches(['"', '\'']);
                if !id.is_empty() && !out.iter().any(|(i, t)| i == id && *t == id_token) {
                    out.push((id.to_owned(), id_token));
                }
            }
        }
    }
    out
}

/// The `code` (and `state`) a redirect to `path` carried, from its request line
/// (`GET /callback?code=…&state=… HTTP/1.1`).
pub fn redirect_code(
    request_line: &str,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = form_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => error = Some(v),
            _ => {}
        }
    }
    Some((code, state, error))
}

/// The fields of a redirect's query, from its request line (`GET /callback?a=1&b=2 …`),
/// decoded; `None`: no query (an Implicit sign-in's answer still in the fragment).
pub fn redirect_fields(request_line: &str) -> Option<Vec<(String, String)>> {
    let target = request_line.split_whitespace().nth(1)?;
    let (_, query) = target.split_once('?')?;
    Some(
        query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                (form_decode(k), form_decode(v))
            })
            .collect(),
    )
}

/// An Implicit sign-in's answer (its fragment's fields, brought back by [`IMPLICIT_PAGE`])
/// read at `now`, when it carries `state`: its token, or the error the server sent. `None`:
/// not this sign-in's.
pub fn implicit_token(
    fields: &[(String, String)],
    state: &str,
    now: u64,
) -> Option<Result<Token, String>> {
    let field = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    if field("state").as_deref() != Some(state) {
        return None;
    }
    if let Some(error) = field("error") {
        let detail = field("error_description").unwrap_or_default();
        return Some(Err(format!("{error} {detail}").trim().to_owned()));
    }
    let access = field("access_token")?;
    Some(Ok(Token {
        access,
        id: field("id_token"),
        refresh: None,
        expires: field("expires_in")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| now.saturating_add(s)),
    }))
}

/// The page an Implicit sign-in's redirect gets first: its fragment (which the browser never
/// sends) brought back to the same address as a query, the page replaced (not kept in the
/// browser's history).
pub const IMPLICIT_PAGE: &str = "<html><body><script>\
var f=location.hash.slice(1);\
if(f){location.replace(location.pathname+'?'+f);}\
else{document.body.textContent='No answer in the address.';}\
</script></body></html>";

fn form_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(text.get(i + 1..i + 3).unwrap_or(""), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[path = "../tests/unit/oauth.rs"]
mod tests;
