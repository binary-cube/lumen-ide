//! What an app's page is served and how it is talked to, apart from the web engine: the
//! scheme its files come from (`local-app://localhost/…`; WebView2 sees it as
//! `https://local-app.localhost/…`), the file a request names (strictly inside the app's
//! folder, else nothing), its type, the content policy that keeps the page off the network,
//! and the script that hands it a message. Pure but for [`serve`], which reads the file
//! (on a worker, never the window's thread).

use std::io::Read;
use std::path::{Path, PathBuf};

/// The scheme an app's files are served through (not `file://`: WebKit and WebView2 refuse
/// a page's requests to it).
pub(crate) const SCHEME: &str = "local-app";

/// The only host the scheme serves.
const HOST: &str = "localhost";

/// The largest file served (bytes): past it, a 404 (an app's scripts are a few MB).
const MAX_FILE: u64 = 128 << 20;

/// What an app's HTML may load and run: its own files only, inline scripts and `eval` (the
/// apps it's for build code at run time), pictures and fonts from data and blobs too; no
/// request leaves the folder, no plugin, no form sent anywhere.
pub(crate) const CSP: &str = "default-src 'self' local-app:; \
     script-src 'self' local-app: 'unsafe-inline' 'unsafe-eval'; \
     style-src 'self' local-app: 'unsafe-inline'; \
     connect-src 'self' local-app: data: blob:; \
     img-src 'self' local-app: data: blob:; \
     font-src 'self' local-app: data:; \
     media-src 'self' local-app: data: blob:; \
     worker-src 'self' local-app: blob:; \
     frame-src 'self' local-app:; \
     object-src 'none'; base-uri 'none'; form-action 'none'";

/// The URL an app is opened at: `entry` (a path in its folder, with its query) under the
/// scheme. On Windows the engine turns it into its `https://local-app.localhost/` form.
pub(crate) fn entry_url(entry: &str) -> String {
    format!("{SCHEME}://{HOST}/{}", entry.trim_start_matches(['/', '\\']))
}

/// Whether `url` is one of the app's own (the scheme's form on macOS, WebView2's on Windows).
pub(crate) fn is_own_url(url: &str) -> bool {
    let own = |prefix: &str| url.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'));
    own(&format!("{SCHEME}://{HOST}"))
        || own(&format!("https://{SCHEME}.{HOST}"))
        || own(&format!("http://{SCHEME}.{HOST}"))
}

/// Whether an app's page may go to `url`: its own files, or an empty frame; nothing else
/// (no network, no other scheme).
pub(crate) fn allows_navigation(url: &str) -> bool {
    is_own_url(url) || url == "about:blank" || url == "about:srcdoc"
}

/// Names Windows reserves for devices (in any folder, with any extension).
const DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// `%XX` escapes decoded (`None`: a broken one).
fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Whether a path's segment names a plain file or folder here, the same on every system:
/// not `.`/`..`, no separator, drive or stream (`\`, `:`), no control character, no
/// Windows device name, no trailing dot or space (Windows would drop them).
fn plain_segment(segment: &str) -> bool {
    if segment.is_empty() || segment == "." || segment == ".." {
        return false;
    }
    if segment.chars().any(|c| c == '\\' || c == ':' || c == '/' || c.is_control()) {
        return false;
    }
    if segment.ends_with('.') || segment.ends_with(' ') {
        return false;
    }
    let stem = segment.split('.').next().unwrap_or(segment).trim_end();
    !DEVICES.iter().any(|d| stem.eq_ignore_ascii_case(d))
}

/// The file a request's path names inside `root` (`None`: outside it, or not a plain
/// path). Decided from the text alone: escapes decoded once, every segment plain (an empty
/// one, from `//`, is skipped). Links that leave the folder on disk are caught by [`serve`].
pub(crate) fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let decoded = String::from_utf8(percent_decode(path)?).ok()?;
    let mut file = root.to_path_buf();
    let mut any = false;
    for segment in decoded.split('/').filter(|s| !s.is_empty()) {
        if !plain_segment(segment) {
            return None;
        }
        file.push(segment);
        any = true;
    }
    any.then_some(file)
}

/// A file's type, by its extension (`application/octet-stream`: one not known).
pub(crate) fn mime(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "xml" => "application/xml",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// What a request is answered with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Served {
    pub(crate) status: u16,
    pub(crate) mime: &'static str,
    pub(crate) body: Vec<u8>,
    /// An HTML page: sent with [`CSP`].
    pub(crate) policy: bool,
}

impl Served {
    fn status(status: u16) -> Self {
        Self { status, mime: "text/plain; charset=utf-8", body: Vec::new(), policy: false }
    }
}

/// The answer to a request for `path` on `host` (`method`: only `GET` and `HEAD` read):
/// the file inside `root`, if it is one there (a link leading out of the folder is not),
/// else a 404. Reads the disk: on a worker.
pub(crate) fn serve(root: &Path, method: &str, host: Option<&str>, path: &str) -> Served {
    let head = method.eq_ignore_ascii_case("HEAD");
    if !head && !method.eq_ignore_ascii_case("GET") {
        return Served::status(405);
    }
    if host.is_none_or(|h| !h.eq_ignore_ascii_case(HOST)) {
        return Served::status(404);
    }
    let Some(file) = resolve(root, path) else { return Served::status(404) };
    // Links followed: what they reach must still be inside the folder.
    let (Ok(root), Ok(real)) = (root.canonicalize(), file.canonicalize()) else {
        tracing::debug!("an app's file isn't there: {path}");
        return Served::status(404);
    };
    if !real.starts_with(&root) {
        tracing::warn!("an app's request left its folder: {}", file.display());
        return Served::status(404);
    }
    let Ok(meta) = std::fs::metadata(&real) else { return Served::status(404) };
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Served::status(404);
    }
    let mime = mime(&real);
    let policy = mime.starts_with("text/html");
    if head {
        return Served { status: 200, mime, body: Vec::new(), policy };
    }
    let mut body = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    let read = std::fs::File::open(&real)
        .and_then(|f| f.take(MAX_FILE.saturating_add(1)).read_to_end(&mut body));
    match read {
        Ok(n) if n as u64 <= MAX_FILE => Served { status: 200, mime, body, policy },
        Ok(_) => Served::status(404),
        Err(e) => {
            tracing::debug!("an app's file wasn't read: {e}");
            Served::status(404)
        }
    }
}

/// `text` as a JavaScript string literal, safe anywhere in a script: JSON's escapes, plus the
/// line separators JavaScript once took as line ends and `<` (no `</script>` closing it).
pub(crate) fn js_string(text: &str) -> String {
    let json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned());
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            '<' => out.push_str("\\u003c"),
            c => out.push(c),
        }
    }
    out
}

/// The script that hands an app's page `message`: its `window.__hostReceive`, if it has one.
pub(crate) fn receive_script(message: &str) -> String {
    format!("window.__hostReceive && window.__hostReceive({});", js_string(message))
}

#[cfg(test)]
#[path = "../tests/unit/web_app.rs"]
mod tests;
