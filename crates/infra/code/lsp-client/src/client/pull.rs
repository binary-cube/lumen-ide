//! Diagnostics asked for (`textDocument/diagnostic`), for the servers that don't push them:
//! once a document is opened or changed, one question at a time per document (changes made
//! meanwhile ask once more when it's answered), the server told what it reported last.

use serde_json::{Value, json};

use super::{Driver, Prepared};
use kernel::DocumentId;
use std::time::Duration;

/// A document's state of asking.
#[derive(Debug, Default)]
pub(super) struct Pull {
    /// What the server called its last report: it answers `unchanged` if still true.
    result_id: Option<String>,
    asking: bool,
    /// It changed while asking: asked again after the answer.
    again: bool,
}

impl Driver {
    /// Asks for `doc`'s diagnostics, if this server is one to ask.
    pub(super) fn pull_diagnostics(&mut self, doc: DocumentId) {
        if !self.capabilities.pull_diagnostics {
            return;
        }
        let Some(uri) = self.docs.get(&doc).map(|d| d.uri.clone()) else { return };
        let state = self.pulls.entry(doc).or_default();
        if state.asking {
            state.again = true;
            return;
        }
        state.asking = true;
        let mut params = json!({"textDocument": {"uri": uri}});
        if let Some(id) = &state.result_id {
            params["previousResultId"] = json!(id);
        }
        let prepared = Prepared { method: "textDocument/diagnostic", params, lines: None };
        let answer = Box::new(move |result: Result<Value, String>, _, driver: &mut Driver| {
            driver.pulled(doc, &uri, result);
        });
        self.request(prepared, Duration::ZERO, answer);
    }

    /// The server asks for every document's again (what it knows changed).
    pub(super) fn pull_all_documents(&mut self) {
        let docs: Vec<DocumentId> = self.docs.keys().copied().collect();
        for doc in docs {
            self.pull_diagnostics(doc);
        }
    }

    fn pulled(&mut self, doc: DocumentId, uri: &str, result: Result<Value, String>) {
        if let Ok(report) = &result {
            self.take_report(uri, report);
            for (related, its) in report["relatedDocuments"].as_object().into_iter().flatten() {
                self.take_report(related, its);
            }
        }
        let Some(state) = self.pulls.get_mut(&doc) else { return };
        state.asking = false;
        if let Ok(report) = &result {
            state.result_id = report["resultId"].as_str().map(str::to_owned);
        }
        if std::mem::take(&mut state.again) {
            self.pull_diagnostics(doc);
        }
    }

    /// A report's findings, as if pushed (`unchanged`: what's shown stays).
    fn take_report(&mut self, uri: &str, report: &Value) {
        if report["kind"] == "full" {
            let items = report.get("items").cloned().unwrap_or_else(|| json!([]));
            self.diagnostics(&json!({"uri": uri, "diagnostics": items}));
        }
    }
}
