//! The JavaScript of a request (plan 77k): `< {% … %}` before it is
//! sent, `> {% … %}` (or `> ./handler.js`) after its response, with the HTTP client API —
//! `client.global` (variables kept between requests), `client.test` / `client.assert` (the
//! response's tests), `client.log`, `request.variables` (this request's own), `request.*`,
//! `response.*`. The engine is a port ([`ScriptEngine`]) its adapter implements; a script
//! reaches nothing but what it's given (no disk, no network, no app).

use std::collections::BTreeMap;

use crate::model::Header;

/// When a script runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before the request is sent (its variables may be set).
    Before,
    /// After its response.
    After,
}

/// The request as a script sees it (as written, its variables not yet put in).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: String,
}

/// The response as a script sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    /// Its body as text (a JSON one is given to the script parsed).
    pub body: String,
    /// Its media type (`application/json`), and its charset if told.
    pub content_type: Option<String>,
    pub charset: Option<String>,
}

/// A test's result (`client.test(name, fn)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    /// `None`: it passed; else why it didn't.
    pub failure: Option<String>,
}

/// What a script is given and what it leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptRun {
    /// `client.global`: what the scripts kept, in (and out, as they left it).
    pub globals: BTreeMap<String, String>,
    /// `request.variables`: what it set for this request (before it's sent).
    pub variables: BTreeMap<String, String>,
    /// `client.log`'s lines.
    pub logs: Vec<String>,
    pub tests: Vec<TestResult>,
    /// Why it stopped (a syntax error, an exception, too long, too much memory).
    pub error: Option<String>,
    /// `client.global.headers`: what it set (`Some`) or cleared (`None`), sent with the
    /// requests after it.
    pub headers: BTreeMap<String, Option<String>>,
    /// Which of a collection's elements this run is for (`request.iteration()`).
    pub iteration: usize,
}

impl ScriptRun {
    /// Every test passed and nothing failed.
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.tests.iter().all(|t| t.failure.is_none())
    }
}

/// Runs a request's scripts (an adapter: a JavaScript engine).
pub trait ScriptEngine: Send + Sync {
    /// `source` run at `phase` for `request` (and `response`, after it), on `run` (its
    /// globals in; what it leaves out).
    fn run(
        &self,
        source: &str,
        phase: Phase,
        request: &ScriptRequest,
        response: Option<&ScriptResponse>,
        run: &mut ScriptRun,
    );
}

#[cfg(test)]
#[path = "../tests/unit/script.rs"]
mod tests;
