//! A response that is a web page, as shown outside the response's text: in the system's
//! browser (Open in Browser) and in the system's web engine inside the app (Preview). The
//! page is the body as it came, with a `<base>` at the address it came from, so what it
//! links to by a relative path (its styles, its pictures) is found there. Made on a worker:
//! a body kept in a file is read.

use std::path::{Path, PathBuf};

use crate::format::{self, Kind};
use crate::runs::Outcome;

/// The largest page shown (the web engines take a page given as text up to about this:
/// WebView2's limit).
pub const LARGEST: usize = 2 * 1024 * 1024;

/// `outcome`'s body as a page to show (`None`: it isn't HTML, or it's larger than
/// [`LARGEST`]).
/// `outcome`'s response can be shown as a page (HTML, not too large).
pub fn shows(outcome: &Outcome) -> bool {
    let response = &outcome.response;
    format::kind(response.media_type().as_deref()) == Kind::Html
        && response.body.len() <= LARGEST as u64
}

pub fn html_of(outcome: &Outcome) -> Option<String> {
    let response = &outcome.response;
    if format::kind(response.media_type().as_deref()) != Kind::Html
        || response.body.len() > LARGEST as u64
    {
        return None;
    }
    let bytes = crate::runs::body_bytes(&response.body);
    Some(with_base(&String::from_utf8_lossy(&bytes), &response.url))
}

/// `html` with a `<base href="url">` (right after its `<head>`, else first), unless it has
/// one of its own.
pub fn with_base(html: &str, url: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if lower.contains("<base ") || lower.contains("<base>") {
        return html.to_owned();
    }
    let href = url.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
    let base = format!("<base href=\"{href}\">");
    // `<head>` or `<head …>`, not `<header>`.
    let head = lower
        .match_indices("<head")
        .find(|(at, _)| lower[at + 5..].starts_with(|c: char| c == '>' || c.is_ascii_whitespace()));
    // After the `<head>`'s `>`; with none, after `<html …>` or the doctype (never before
    // the doctype: the page would be read in quirks mode).
    let after_tag = |tag: &str| {
        let at = lower.find(tag)?;
        lower[at..].find('>').map(|end| at + end + 1)
    };
    let after = head
        .and_then(|(at, _)| lower[at..].find('>').map(|end| at + end + 1))
        .or_else(|| after_tag("<html"))
        .or_else(|| after_tag("<!doctype"));
    match after {
        Some(at) => format!("{}{base}{}", &html[..at], &html[at..]),
        None => format!("{base}{html}"),
    }
}

/// `html` written for the browser in `folder` (`pages/`, the newest few kept).
pub fn write(folder: &Path, html: &str) -> Option<PathBuf> {
    let dir = folder.join("pages");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{:08x}.html", kernel::id::new_uuid() as u32));
    files::write_atomic(&path, html.as_bytes()).ok()?;
    crate::job::keep_newest(&dir);
    Some(path)
}

/// `html` written for the browser beside the project's responses (`folder`), and the app
/// asked to open it (on a worker: it writes).
pub fn open_in_browser(folder: &Path, html: &str) -> Option<ui_components::ViewerAction> {
    crate::job::ready_folder(folder);
    let path = write(folder, html)?;
    Some(ui_components::ViewerAction::Request(ui_components::AppRequest::OpenInBrowser(path)))
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
