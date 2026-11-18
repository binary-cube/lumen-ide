//! What a `.http` file holds: its variables and its requests, each as written (its variables
//! not resolved yet: [`crate::vars`]), with where it is in the file.

use std::ops::Range;
use std::time::Duration;

/// A `.http` file read ([`crate::parse`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpFile {
    /// Its own variables (`@host = example.com`), in order: a later one wins.
    pub variables: Vec<FileVariable>,
    pub requests: Vec<Request>,
}

impl HttpFile {
    /// The request at byte `offset` (the one whose block holds it).
    pub fn request_at(&self, offset: usize) -> Option<&Request> {
        self.requests.get(self.index_at(offset)?)
    }

    /// [`Self::request_at`]'s index.
    pub fn index_at(&self, offset: usize) -> Option<usize> {
        self.requests.iter().position(|r| r.range.contains(&offset) || r.range.end == offset)
    }

    /// The request named `name` (`# @name`, or the `###` title).
    pub fn named(&self, name: &str) -> Option<&Request> {
        self.requests.get(self.index_of(name)?)
    }

    /// [`Self::named`]'s index.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.requests.iter().position(|r| r.name.as_deref() == Some(name))
    }
}

/// `@name = value`: a variable of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVariable {
    pub name: String,
    pub value: String,
    /// Its line (0-based).
    pub line: u32,
}

/// A request as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// What other requests call it by: `# @name login`, else its `###` title.
    pub name: Option<String>,
    /// The text after its `###` separator, when there's one.
    pub title: Option<String>,
    pub method: String,
    /// As written, its continuation lines (`?a=1`, `&b=2`) joined.
    pub url: String,
    /// `HTTP/1.1`, `HTTP/2`, when written.
    pub version: Option<String>,
    pub headers: Vec<Header>,
    pub body: Vec<BodyPart>,
    pub options: Options,
    /// Run before it: `< {% … %}` / `< script.js` above its request line.
    pub pre_script: Option<Script>,
    /// Run with its response: `> {% … %}` / `> handler.js` after its body.
    pub post_script: Option<Script>,
    /// Where its response is written too: `>> file` (a new name if it's taken), `>>! file`
    /// (replaced).
    pub save_to: Option<SaveTo>,
    /// Its block in the file (bytes): from after the separator before it to the next one.
    pub range: Range<usize>,
    /// Its request line (0-based): where its ▷ goes.
    pub line: u32,
    /// That line's bytes in the file (without its end).
    pub line_range: Range<usize>,
}

impl Request {
    /// How it's shown: its name, else its method and URL.
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("{} {}", self.method, self.url),
        }
    }

    /// Its header `name` (any case), the last one written.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().rev().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| &*h.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A piece of a request's body: text as written, or a file's content (`< ./body.json`,
/// relative to the `.http` file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart {
    Text(String),
    File(String),
}

/// A script: written in the file between `{%` and `%}`, or a file of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Script {
    Inline(String),
    File(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTo {
    pub path: String,
    /// `>>!`: replaced rather than written beside under a new name.
    pub replace: bool,
}

/// What its comments ask (`# @no-redirect`, `# @timeout 5`…).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Redirects aren't followed: the redirect itself is the response.
    pub no_redirect: bool,
    /// The project's cookies aren't sent nor kept.
    pub no_cookie_jar: bool,
    /// Not written in the history (a response with secrets).
    pub no_log: bool,
    /// For the whole response.
    pub timeout: Option<Duration>,
    /// For the connection only.
    pub connection_timeout: Option<Duration>,
    /// Its URL and body sent exactly as written (no percent-encoding of what isn't allowed).
    pub no_auto_encoding: bool,
}
