//! OAuth 2.0 tokens for `{{$auth.token("id")}}` (plan 77j), had on the run's worker: a
//! token kept fresh is used as it is; an expired one refreshed; otherwise one acquired by
//! the configuration's grant — Client Credentials and Password asked for directly,
//! Authorization Code (PKCE as configured) by the user signing in in the browser, the
//! answer caught on the Redirect URL's port, Device Authorization by the user entering its
//! code. Tokens are kept per project and environment while the project is open; one
//! configuration is had by one run at a time (another waits for its token, never opens a
//! second sign-in).
//!
//! The redirect's listener answers only on this machine, reads at most a line from each
//! asker (and not for long), and ends the sign-in only on the answer carrying its state:
//! anything else (a browser's icon, a stray page) is told "not found" and waited past.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv6Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use http_requests::oauth::{self, AuthConfig, FormPost, Grant, Token};
use http_requests::transport::{HttpTransport, Outgoing, OutgoingPart, SendOptions};
use kernel::work::Cancel;

/// How long the user has to sign in.
const SIGN_IN: Duration = Duration::from_secs(180);
/// How long an asker of the redirect's port has to send its request line, and how long
/// it may be.
const ASKER: Duration = Duration::from_secs(2);
const LINE: u64 = 8 * 1024;
/// The longest a device flow waits between asks, and in all (whatever the server says).
const DEVICE_INTERVAL: u64 = 60;
const DEVICE_EXPIRES: u64 = 30 * 60;
/// How often a wait looks whether it was cancelled.
const STEP: Duration = Duration::from_millis(100);

type Key = (Option<String>, String);

/// A project's tokens, by environment and configuration.
#[derive(Debug, Default)]
pub struct Tokens {
    kept: Mutex<HashMap<Key, Token>>,
    /// One lock per configuration: who has it gets its token, the others wait for it.
    getting: Mutex<HashMap<Key, Arc<Mutex<()>>>>,
}

impl Tokens {
    fn lock_of(&self, key: &Key) -> Arc<Mutex<()>> {
        let mut getting = self.getting.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(getting.entry(key.clone()).or_default())
    }

    fn get(&self, key: &Key) -> Option<Token> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
    }

    fn put(&self, key: Key, token: Token) {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner).insert(key, token);
    }

    /// Every token let go (the user asked for new ones).
    pub fn clear(&self) {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

/// What getting a token needs from its run: sending, the time, opening the browser, telling
/// the user, a stop.
pub struct Context<'a> {
    pub transport: &'a dyn HttpTransport,
    pub now: u64,
    /// A page opened in the system's browser (the sign-in, the device's page).
    pub open: &'a dyn Fn(&str),
    /// A message for the user (the device's code to enter).
    pub tell: &'a dyn Fn(&str),
    pub cancel: &'a Cancel,
}

/// A page the browser may be sent to: the web's only (never a file, a program's scheme).
pub(crate) fn web_page(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// The access token (or ID token) of configuration `config` (named `id`, in `environment`).
pub fn token(
    tokens: &Tokens,
    environment: Option<&str>,
    id: &str,
    config: &AuthConfig,
    id_token: bool,
    ctx: &Context<'_>,
) -> Result<String, String> {
    let pick = |token: &Token| -> Result<String, String> {
        if id_token || config.use_id_token {
            token.id.clone().ok_or_else(|| "the answer has no id_token".to_owned())
        } else {
            Ok(token.access.clone())
        }
    };
    if let Some((access, id)) = &config.mock {
        return Ok(if id_token { id.clone().unwrap_or_default() } else { access.clone() });
    }
    let key = (environment.map(str::to_owned), id.to_owned());
    // Another run getting it: waited for, then its token used.
    let lock = tokens.lock_of(&key);
    let _getting = lock.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(kept) = tokens.get(&key) {
        if kept.fresh(ctx.now) {
            return pick(&kept);
        }
        if let Some(refresh) = &kept.refresh
            && let Ok(mut fresh) =
                post(ctx, &config.refresh(refresh)).and_then(|b| oauth::read_token(&b, ctx.now))
        {
            fresh.refresh = fresh.refresh.or_else(|| kept.refresh.clone());
            tokens.put(key, fresh.clone());
            return pick(&fresh);
        }
    }
    let token = match config.grant {
        Grant::ClientCredentials => {
            oauth::read_token(&post(ctx, &config.client_credentials())?, ctx.now)?
        }
        Grant::Password => oauth::read_token(&post(ctx, &config.password_grant())?, ctx.now)?,
        Grant::AuthorizationCode => authorization_code(config, ctx)?,
        Grant::DeviceAuthorization => device(config, ctx)?,
        Grant::Implicit => implicit(config, ctx)?,
    };
    tokens.put(key, token.clone());
    pick(&token)
}

/// A form posted to an OAuth endpoint: its answer's text.
fn post(ctx: &Context<'_>, form: &FormPost) -> Result<String, String> {
    if form.url.is_empty() {
        return Err("the configuration has no endpoint for that".to_owned());
    }
    let mut headers: Vec<http_requests::Header> = form
        .headers
        .iter()
        .map(|(n, v)| http_requests::Header { name: n.clone(), value: v.clone() })
        .collect();
    headers.push(http_requests::Header {
        name: "Content-Type".into(),
        value: "application/x-www-form-urlencoded".into(),
    });
    let outgoing = Outgoing {
        method: "POST".into(),
        url: form.url.clone(),
        headers,
        body: vec![OutgoingPart::Bytes(form.body().into_bytes())],
    };
    let response = ctx
        .transport
        .send(&outgoing, &SendOptions::default(), None, ctx.cancel)
        .map_err(|e| e.to_string())?;
    let bytes = crate::runs::body_bytes(&response.body);
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Authorization Code: the sign-in page opened, its redirect caught on the Redirect URL's
/// port (or one of our own), the code exchanged.
fn authorization_code(config: &AuthConfig, ctx: &Context<'_>) -> Result<Token, String> {
    let (listener, redirect) = listen(&config.redirect_url)?;
    let state = kernel::id::uuid_text(kernel::id::new_uuid());
    let verifier: String = (0..2).map(|_| format!("{:032x}", kernel::id::new_uuid())).collect();
    (ctx.open)(&config.authorize_url(&redirect, &state, &verifier));
    let started = Instant::now();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    loop {
        if ctx.cancel.is_cancelled() {
            return Err(i18n::t("http.cancelled"));
        }
        if started.elapsed() > SIGN_IN {
            return Err(i18n::t("http.sign_in_timeout"));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(answer) = redirect_answer(stream, &state) {
                    let code = answer?;
                    return oauth::read_token(
                        &post(ctx, &config.exchange(&code, &redirect, &verifier))?,
                        ctx.now,
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(STEP / 2),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Implicit: the sign-in page opened, its redirect caught as for a code; the token comes in
/// the address's fragment, which the browser never sends: a page of ours sends it back as a
/// query ([`oauth::IMPLICIT_PAGE`]), and the token is read from that.
fn implicit(config: &AuthConfig, ctx: &Context<'_>) -> Result<Token, String> {
    let (listener, redirect) = listen(&config.redirect_url)?;
    let state = kernel::id::uuid_text(kernel::id::new_uuid());
    (ctx.open)(&config.authorize_url(&redirect, &state, ""));
    let started = Instant::now();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    loop {
        if ctx.cancel.is_cancelled() {
            return Err(i18n::t("http.cancelled"));
        }
        if started.elapsed() > SIGN_IN {
            return Err(i18n::t("http.sign_in_timeout"));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(answer) = implicit_answer(stream, &state, ctx.now) {
                    return answer;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(STEP / 2),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// What one asker of an Implicit redirect's port brought: the page that sends the fragment
/// back (`None`: waited on), another page's ask (`None`), or this sign-in's token or error.
fn implicit_answer(stream: TcpStream, state: &str, now: u64) -> Option<Result<Token, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(ASKER));
    let mut line = String::new();
    let mut reader = BufReader::new(stream.try_clone().ok()?.take(LINE));
    let _ = reader.read_line(&mut line);
    let Some(fields) = oauth::redirect_fields(&line) else {
        page(&stream, "200 OK", oauth::IMPLICIT_PAGE);
        return None;
    };
    let Some(answer) = oauth::implicit_token(&fields, state, now) else {
        page(&stream, "404 Not Found", "");
        return None;
    };
    match &answer {
        Ok(_) => page(&stream, "200 OK", &signed_page(&i18n::t("http.signed_in"))),
        Err(why) => page(&stream, "400 Bad Request", &signed_page(&html_text(why))),
    }
    Some(answer)
}

/// A page answered on the redirect's port.
fn page(mut stream: &TcpStream, status: &str, page: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
}

/// The page saying how the sign-in went (`text`: already safe as markup).
fn signed_page(text: &str) -> String {
    format!("<html><body style=\"font-family:sans-serif;margin:3em\"><h2>{text}</h2></body></html>")
}

/// What one asker of the redirect's port brought: `None` not the sign-in's answer (told
/// "not found", waited past); else its code, or the error the server sent back.
fn redirect_answer(stream: TcpStream, state: &str) -> Option<Result<String, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(ASKER));
    let mut line = String::new();
    let mut reader = BufReader::new(stream.try_clone().ok()?.take(LINE));
    let _ = reader.read_line(&mut line);
    let reply = |stream: &TcpStream, status: &str, text: &str| {
        page(stream, status, &signed_page(text));
    };
    // Only the answer with this sign-in's state: another page's (or no state) isn't it.
    let ours = oauth::redirect_code(&line)
        .filter(|(_, got, _)| got.as_deref() == Some(state))
        .filter(|(code, _, error)| code.is_some() || error.is_some());
    let Some((code, _, error)) = ours else {
        reply(&stream, "404 Not Found", "");
        return None;
    };
    if let Some(error) = error {
        reply(&stream, "400 Bad Request", &html_text(&error));
        return Some(Err(error));
    }
    reply(&stream, "200 OK", &i18n::t("http.signed_in"));
    code.map(Ok)
}

/// A text shown in a page as text (never as its markup).
fn html_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Where the redirect is caught: the Redirect URL's host and port (on this machine), else a
/// port of our own; the redirect URL to give.
fn listen(redirect_url: &str) -> Result<(TcpListener, String), String> {
    if let Some(url) = http_requests::url::Url::parse(redirect_url) {
        let host = url.host.trim_start_matches('[').trim_end_matches(']');
        let local = matches!(host, "localhost" | "127.0.0.1" | "::1");
        if local && url.scheme == "http" {
            let port = url.port_or_default();
            let bound = if host == "::1" {
                TcpListener::bind((Ipv6Addr::LOCALHOST, port))
            } else {
                TcpListener::bind(("127.0.0.1", port))
            };
            let listener = bound.map_err(|e| format!("port {port}: {e}"))?;
            return Ok((listener, redirect_url.to_owned()));
        }
        if !redirect_url.is_empty() {
            return Err(format!(
                "the Redirect URL must be on this machine (http://localhost:…): {redirect_url}"
            ));
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok((listener, format!("http://127.0.0.1:{port}/callback")))
}

/// Device Authorization: the user's code told and its page opened, the token endpoint
/// asked until it answers.
fn device(config: &AuthConfig, ctx: &Context<'_>) -> Result<Token, String> {
    let first = oauth::read_device(&post(ctx, &config.device_request())?)?;
    if web_page(&first.verification_uri) {
        if !first.user_code.is_empty() {
            (ctx.tell)(&i18n::tr!(
                "http.device_code",
                code = first.user_code.as_str(),
                page = first.verification_uri.as_str()
            ));
        }
        (ctx.open)(&first.verification_uri);
    } else {
        return Err(format!("not a web page to sign in at: {}", first.verification_uri));
    }
    let started = Instant::now();
    let expires = Duration::from_secs(first.expires_in.min(DEVICE_EXPIRES));
    let mut interval = Duration::from_secs(first.interval.clamp(1, DEVICE_INTERVAL));
    loop {
        let waited = Instant::now();
        while waited.elapsed() < interval {
            if ctx.cancel.is_cancelled() {
                return Err(i18n::t("http.cancelled"));
            }
            std::thread::sleep(STEP);
        }
        if started.elapsed() > expires {
            return Err(i18n::t("http.sign_in_timeout"));
        }
        let body = post(ctx, &config.device_poll(&first.device_code))?;
        match oauth::read_token(&body, ctx.now) {
            Ok(token) => return Ok(token),
            Err(why) if why.starts_with("authorization_pending") => {}
            Err(why) if why.starts_with("slow_down") => {
                interval =
                    (interval + Duration::from_secs(5)).min(Duration::from_secs(DEVICE_INTERVAL));
            }
            Err(why) => return Err(why),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/auth.rs"]
mod tests;
