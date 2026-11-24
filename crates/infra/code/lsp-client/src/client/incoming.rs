//! What a server sends.

use super::*;

impl Driver {
    pub(super) fn incoming(&mut self, message: Message) {
        if self.config.trace.load(Ordering::Relaxed) {
            let text = self.status.redacted(&message.to_value().to_string());
            self.status.log(format!("← {}", traced(&text)));
        }
        match message {
            Message::Response { id: Id::Number(id), result } => {
                if self.instance.as_ref().and_then(|i| i.initializing) == Some(id) {
                    self.initialized(result);
                    return;
                }
                let Some(mut pending) = self.pending.remove(&id) else { return };
                self.status.answered(pending.sent.elapsed());
                let result = result.map_err(|e| {
                    if e.code != codes::REQUEST_CANCELLED && e.code != codes::CONTENT_MODIFIED {
                        self.status.error(format!("{}: {}", pending.method, e.message));
                    }
                    e.message
                });
                if let Some(answer) = pending.answer.take() {
                    answer(result, pending.lines.take(), self);
                }
            }
            Message::Response { .. } => {}
            Message::Request { id, method, params } => self.server_request(id, &method, params),
            Message::Notification { method, params } => self.server_notification(&method, params),
        }
    }

    /// A request of the server's.
    pub(super) fn server_request(&mut self, id: Id, method: &str, params: Value) {
        let result = match method {
            "workspace/configuration" => {
                let items =
                    params.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
                let answers: Vec<Value> = items
                    .iter()
                    .map(|item| {
                        section(&self.config.settings, item.get("section").and_then(Value::as_str))
                    })
                    .collect();
                Ok(Value::Array(answers))
            }
            "workspace/workspaceFolders" => {
                let root = uri::from_path(&self.config.root);
                let name = self
                    .config
                    .root
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                Ok(json!([{"uri": root, "name": name}]))
            }
            "workspace/applyEdit" => {
                let label = params.get("label").and_then(Value::as_str).map(str::to_owned);
                let change = params.get("edit").map(|e| self.workspace_edit(e)).unwrap_or_default();
                self.event(Event::ApplyEdit { label, change });
                Ok(json!({"applied": true}))
            }
            "vscode/content" => self.content(&params),
            "window/showMessageRequest" => {
                let message = params.get("message").and_then(Value::as_str).unwrap_or_default();
                self.status.log(format!("message: {message}"));
                Ok(Value::Null)
            }
            "window/showDocument" => Ok(json!({"success": false})),
            "window/workDoneProgress/create"
            | "client/registerCapability"
            | "client/unregisterCapability"
            | "workspace/semanticTokens/refresh"
            | "workspace/inlayHint/refresh"
            | "workspace/codeLens/refresh" => Ok(Value::Null),
            "workspace/diagnostic/refresh" => {
                self.pull_all_documents();
                Ok(Value::Null)
            }
            _ => Err(ResponseError {
                code: codes::METHOD_NOT_FOUND,
                message: format!("{method} isn't handled"),
            }),
        };
        self.respond(id, result);
    }

    /// The text at a URL the server asks for (`vscode/content`: a schema), from the folders
    /// it's given; any other URL is refused (nothing is ever downloaded).
    pub(super) fn content(&mut self, params: &Value) -> Result<Value, ResponseError> {
        let url = match params {
            Value::String(url) => url.as_str(),
            Value::Array(items) => items.first().and_then(Value::as_str).unwrap_or_default(),
            other => other.get("uri").and_then(Value::as_str).unwrap_or_default(),
        };
        let refused = |message: String| ResponseError { code: codes::INVALID_PARAMS, message };
        let Some(file) = crate::spec::content_file(&self.config.contents, url) else {
            self.status.log(format!("content refused (not bundled, never downloaded): {url}"));
            return Err(refused(format!("{url}: not bundled (downloads are off)")));
        };
        match std::fs::read_to_string(&file) {
            Ok(text) => Ok(Value::String(text)),
            Err(e) => {
                self.status.log(format!("content not found: {url} ({e})"));
                Err(refused(format!("{url}: not bundled (downloads are off)")))
            }
        }
    }

    /// A notification of the server's.
    pub(super) fn server_notification(&mut self, method: &str, params: Value) {
        match method {
            "textDocument/publishDiagnostics" => self.diagnostics(&params),
            "$/progress" => {
                let token = match params.get("token") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => return,
                };
                let value = params.get("value").cloned().unwrap_or(Value::Null);
                let percentage =
                    value.get("percentage").and_then(Value::as_u64).map(|p| p.min(100) as u32);
                match value.get("kind").and_then(Value::as_str) {
                    Some("begin") => {
                        let title = value
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        self.progress_titles.insert(token.clone(), title.clone());
                        self.status.progress(&token, Some((title, percentage)));
                    }
                    Some("report") => {
                        let title = self.progress_titles.get(&token).cloned().unwrap_or_default();
                        self.status.progress(&token, Some((title, percentage)));
                    }
                    Some("end") => {
                        self.progress_titles.remove(&token);
                        self.status.progress(&token, None);
                    }
                    _ => return,
                }
                self.event(Event::Changed);
            }
            "window/logMessage" | "window/showMessage" => {
                let message = params.get("message").and_then(Value::as_str).unwrap_or_default();
                let kind = params.get("type").and_then(Value::as_u64).unwrap_or(4);
                if kind == 1 && method == "window/showMessage" {
                    self.status.error(message);
                } else {
                    self.status.log(message);
                }
                if method == "window/showMessage" && self.config.show_messages && kind <= 3 {
                    let kind = match kind {
                        1 => MessageKind::Error,
                        2 => MessageKind::Warning,
                        _ => MessageKind::Info,
                    };
                    self.event(Event::Message { kind, text: message.to_owned() });
                }
            }
            _ => {}
        }
    }

    pub(super) fn diagnostics(&mut self, params: &Value) {
        let Some(uri_text) = params.get("uri").and_then(Value::as_str) else { return };
        let Some(path) = uri::to_path(uri_text) else { return };
        let Ok(relative) = path.strip_prefix(&self.config.project_root) else { return };
        let items = params.get("diagnostics").cloned().unwrap_or(json!([]));
        // None: nothing to place in its text (a file not open isn't read for it).
        let diagnostics = if items.as_array().is_none_or(Vec::is_empty) {
            Vec::new()
        } else {
            let mut texts = DriverTexts::new(&self.docs);
            let Some(lines) = texts.lines(&path) else { return };
            convert::diagnostics(&items, relative, &lines, self.encoding, &self.config.name)
        };
        // As sent: a code action there sends them back.
        match items {
            Value::Array(raw) if !raw.is_empty() => {
                self.published.insert(uri_text.to_owned(), raw);
                if self.published.len() > super::PUBLISHED_MOST {
                    let open: std::collections::HashSet<&str> =
                        self.docs.values().map(|d| d.uri.as_str()).collect();
                    self.published.retain(|uri, _| uri == uri_text || open.contains(uri.as_str()));
                }
            }
            _ => {
                self.published.remove(uri_text);
            }
        }
        self.event(Event::Diagnostics { path: relative.to_path_buf(), diagnostics });
    }
}
