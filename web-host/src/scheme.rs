//! The app scheme (`local-app://localhost/…`): an app page's files, only from its own folder,
//! with the content policy that keeps it off the network — the rules of `web_pages`, the
//! same as the system's engine. Declared in every process (CEF asks each), served in the
//! host on CEF's IO thread (a file read there, never on the main thread).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use cef::rc::Rc as _;
use cef::*;
use web_pages::Root;

/// Each page's folder, by its browser's identity (set as a page opens, gone as it closes):
/// shared by its requests, its place on disk found once.
static ROOTS: Mutex<Option<HashMap<i32, Arc<Root>>>> = Mutex::new(None);

/// Each HTML page's text, by its token (the path of its address: random, so no other page
/// finds it), served from memory whatever its size.
static HTML: Mutex<Option<HashMap<String, Arc<str>>>> = Mutex::new(None);

/// Where HTML pages are: a host of the scheme of their own, never an app's files (those are
/// on [`web_pages::HOST`]).
const HTML_HOST: &str = "page";

/// The HTML pages' policy: nothing runs, nothing is fetched (pictures and fonts only from
/// the page itself), styles inline only.
const HTML_CSP: &str = "default-src 'none'; img-src data:; font-src data:; \
     style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'";

/// An HTML page's text kept under a new token: its address. The page opens there directly
/// (never first on a blank page, which the engine would keep showing).
pub fn put_html(html: String) -> HtmlPage {
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut pages = HTML.lock().unwrap_or_else(PoisonError::into_inner);
    pages.get_or_insert_with(HashMap::new).insert(token.clone(), Arc::from(html));
    HtmlPage { url: html_url(&token), token }
}

/// An HTML page's address, by its token.
fn html_url(token: &str) -> String {
    format!("{}://{HTML_HOST}/{token}", web_pages::SCHEME)
}

/// An HTML page kept ([`put_html`]): its address; its text let go when this drops.
#[derive(Debug)]
pub struct HtmlPage {
    pub url: String,
    token: String,
}

impl Drop for HtmlPage {
    fn drop(&mut self) {
        let mut pages = HTML.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pages) = pages.as_mut() {
            pages.remove(&self.token);
        }
    }
}

fn html_of(token: &str) -> Option<Arc<str>> {
    HTML.lock().unwrap_or_else(PoisonError::into_inner).as_ref()?.get(token).cloned()
}

/// The folder browser `browser` is served from.
pub fn set_root(browser: i32, root: Option<PathBuf>) {
    let mut roots = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
    let roots = roots.get_or_insert_with(HashMap::new);
    match root {
        Some(root) => roots.insert(browser, Arc::new(Root::new(root))),
        None => roots.remove(&browser),
    };
}

fn root_of(browser: i32) -> Option<Arc<Root>> {
    ROOTS.lock().unwrap_or_else(PoisonError::into_inner).as_ref()?.get(&browser).cloned()
}

/// The scheme declared to CEF (in every process: renderers check it too): standard, secure
/// (an app's scripts get what secure pages get), its own requests and fetches allowed.
pub fn declare(registrar: Option<&mut SchemeRegistrar>) {
    let Some(registrar) = registrar else { return };
    let options = SchemeOptions::STANDARD.get_raw()
        | SchemeOptions::SECURE.get_raw()
        | SchemeOptions::CORS_ENABLED.get_raw()
        | SchemeOptions::FETCH_ENABLED.get_raw();
    registrar.add_custom_scheme(Some(&web_pages::SCHEME.into()), options as _);
}

/// The scheme served (in the host, once CEF started): apps' files on [`web_pages::HOST`],
/// HTML pages on [`HTML_HOST`].
pub fn register() {
    for domain in [web_pages::HOST, HTML_HOST] {
        let mut factory = FactoryBuilder::new(Factory);
        register_scheme_handler_factory(
            Some(&web_pages::SCHEME.into()),
            Some(&domain.into()),
            Some(&mut factory),
        );
    }
}

#[derive(Debug, Clone)]
pub struct Factory;

wrap_scheme_handler_factory! {
    pub struct FactoryBuilder {
        factory: Factory,
    }

    impl SchemeHandlerFactory {
        fn create(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            let browser = browser?.identifier();
            let url = crate::text_of(&request?.url());
            let content = match split_url(&url) {
                // An HTML page: its text, found by its token.
                (Some(HTML_HOST), token) => Content::Html(html_of(token)?),
                _ => Content::Folder(root_of(browser)?),
            };
            Some(FileBuilder::new(File { content, served: Arc::default() }))
        }
    }
}

/// A request's answer, and how much of its body was read.
#[derive(Default)]
struct Answer {
    served: Option<Reply>,
    at: usize,
}

/// What a request is answered with.
struct Reply {
    status: u16,
    mime: &'static str,
    /// Sent with its content policy (an HTML page).
    policy: bool,
    body: Body,
}

/// A reply's body: a file read, or an HTML page's text, shared with its page (never copied
/// per request).
enum Body {
    Read(Vec<u8>),
    Html(Arc<str>),
}

impl Body {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Read(bytes) => bytes,
            Self::Html(html) => html.as_bytes(),
        }
    }
}

impl From<web_pages::Served> for Reply {
    fn from(served: web_pages::Served) -> Self {
        let web_pages::Served { status, mime, body, policy } = served;
        Self { status, mime, policy, body: Body::Read(body) }
    }
}

/// What a page's requests are answered from.
#[derive(Clone)]
enum Content {
    /// An app's folder.
    Folder(Arc<Root>),
    /// An HTML page's text (only its own address answers).
    Html(Arc<str>),
}

#[derive(Clone)]
pub struct File {
    content: Content,
    served: Arc<Mutex<Answer>>,
}

impl std::fmt::Debug for File {
    // A page's content is never told.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("File").finish_non_exhaustive()
    }
}

impl File {
    fn answer(&self) -> std::sync::MutexGuard<'_, Answer> {
        self.served.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

wrap_resource_handler! {
    pub struct FileBuilder {
        file: File,
    }

    impl ResourceHandler {
        /// Read at once (we're on CEF's IO thread: a file read is fine here).
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            let Some(request) = request else { return 0 };
            let url = crate::text_of(&request.url());
            let method = crate::text_of(&request.method());
            let (host, path) = split_url(&url);
            let served = match &self.file.content {
                Content::Folder(root) => Reply::from(root.serve(&method, host, path)),
                Content::Html(html) => Reply {
                    status: 200,
                    mime: "text/html; charset=utf-8",
                    policy: true,
                    body: Body::Html(Arc::clone(html)),
                },
            };
            tracing::debug!(status = served.status, bytes = served.body.bytes().len(), "a page's resource served");
            self.file.answer().served = Some(served);
            if let Some(handle) = handle_request {
                *handle = 1;
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let answer = self.file.answer();
            let Some(served) = &answer.served else { return };
            if let Some(response) = response {
                response.set_status(i32::from(served.status));
                // CEF takes the type alone, its charset apart (`text/html; charset=utf-8` whole
                // isn't a type it knows: the page would show as text).
                let (mime, charset) = split_mime(served.mime);
                response.set_mime_type(Some(&mime.into()));
                if let Some(charset) = charset {
                    response.set_charset(Some(&charset.into()));
                }
                if served.policy {
                    let policy = match self.file.content {
                        Content::Folder(_) => web_pages::CSP,
                        Content::Html(_) => HTML_CSP,
                    };
                    response.set_header_by_name(
                        Some(&"Content-Security-Policy".into()),
                        Some(&policy.into()),
                        1,
                    );
                }
            }
            if let Some(length) = response_length {
                *length = i64::try_from(served.body.bytes().len()).unwrap_or(-1);
            }
        }

        // CEF's callback hands a raw buffer: its shape is CEF's, not ours to mark `unsafe`.
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let mut answer = self.file.answer();
            let at = answer.at;
            let Some(served) = &answer.served else { return 0 };
            let body = served.body.bytes();
            let left = body.len().saturating_sub(at);
            let n = left.min(usize::try_from(bytes_to_read).unwrap_or(0));
            if n == 0 || data_out.is_null() {
                return 0;
            }
            // SAFETY: CEF gives `bytes_to_read` writable bytes at `data_out`; `n` fits both.
            #[allow(unsafe_code)]
            unsafe { std::ptr::copy_nonoverlapping(body[at..].as_ptr(), data_out, n) };
            answer.at = at + n;
            if let Some(read) = bytes_read {
                *read = i32::try_from(n).unwrap_or(0);
            }
            1
        }
    }
}

/// A content type as CEF takes it: the type, and its charset if it names one.
fn split_mime(mime: &str) -> (&str, Option<&str>) {
    let mut parts = mime.split(';').map(str::trim);
    let kind = parts.next().unwrap_or(mime);
    let charset = parts.find_map(|p| p.strip_prefix("charset="));
    (kind, charset)
}

/// `local-app://host/path?query` → (`host`, `/path`) (the query isn't part of the file).
fn split_url(url: &str) -> (Option<&str>, &str) {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    match rest.split_once('/') {
        Some((host, path)) => (Some(host), path),
        None => (Some(rest), ""),
    }
}

#[cfg(test)]
#[path = "../tests/unit/scheme.rs"]
mod tests;
