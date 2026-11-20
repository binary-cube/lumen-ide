//! A server started, initialized, restarted and stopped.

use super::*;

impl Driver {
    pub(super) fn start_instance(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        self.status.set_state(State::Starting);
        self.event(Event::Changed);
        let connection = match self.launcher.launch() {
            Ok(c) => c,
            Err(e) => {
                self.status.error(format!("it couldn't be started: {e}"));
                self.status.set_state(State::Failed(e.to_string()));
                self.event(Event::Changed);
                return;
            }
        };
        let Connection { input, output, errors, pid, keep } = connection;
        let name = self.config.name.clone();
        let writer = spawn_writer(&name, input);
        spawn_reader(&name, output, self.tx.clone(), generation);
        if let Some(errors) = errors {
            spawn_errors(&name, errors, self.tx.clone(), generation);
        }
        let started = Instant::now();
        self.status.set_process(pid, Some(started));
        self.status
            .log(format!("started{}", pid.map(|p| format!(" (pid {p})")).unwrap_or_default()));
        self.instance = Some(Instance { writer, _process: keep, initializing: None, started });
        let id = self.next();
        let mut params = self.initialize_params();
        let options = self.secret_options();
        if !options.is_empty() {
            if !params["initializationOptions"].is_object() {
                params["initializationOptions"] = json!({});
            }
            for (key, value) in options {
                params["initializationOptions"][key] = value;
            }
        }
        self.write(&Message::Request { id: Id::Number(id), method: "initialize".into(), params });
        if let Some(instance) = &mut self.instance {
            instance.initializing = Some(id);
        }
    }

    /// The initialization options its secrets give (read now, from the store), each kept out
    /// of its log from now on.
    pub(super) fn secret_options(&self) -> Vec<(String, Value)> {
        let Some(store) = &self.config.secrets else { return Vec::new() };
        let mut given = Vec::new();
        let options = self
            .config
            .secret_options
            .iter()
            .filter_map(|(key, id)| {
                let secret = store.get(id).filter(|s| !s.expose().trim().is_empty())?;
                let value = Value::String(secret.expose().trim().to_owned());
                given.push(secret);
                Some((key.clone(), value))
            })
            .collect();
        self.status.redact(given);
        options
    }

    pub(super) fn initialize_params(&self) -> Value {
        let root = uri::from_path(&self.config.root);
        let name = self
            .config
            .root
            .file_name()
            .map_or_else(|| "root".to_owned(), |n| n.to_string_lossy().into_owned());
        json!({
            "processId": std::process::id(),
            "clientInfo": {"name": kernel::brand::NAME, "version": kernel::brand::version()},
            "rootUri": root,
            "rootPath": self.config.root.to_string_lossy(),
            "workspaceFolders": [{"uri": root, "name": name}],
            "initializationOptions": self.config.initialization_options,
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16", "utf-32"]},
                "textDocument": {
                    "synchronization": {"didSave": true, "willSave": false, "willSaveWaitUntil": false, "dynamicRegistration": false},
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true,
                            "insertReplaceSupport": true,
                            "labelDetailsSupport": true,
                            "deprecatedSupport": true,
                            "documentationFormat": ["markdown", "plaintext"],
                            "resolveSupport": {"properties": ["additionalTextEdits", "detail", "documentation"]},
                        },
                        "contextSupport": true,
                        "completionList": {"itemDefaults": ["editRange", "insertTextFormat", "data", "commitCharacters"]},
                    },
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "signatureHelp": {
                        "signatureInformation": {
                            "documentationFormat": ["markdown", "plaintext"],
                            "parameterInformation": {"labelOffsetSupport": true},
                            "activeParameterSupport": true,
                        },
                        "contextSupport": true,
                    },
                    "definition": {"linkSupport": true},
                    "implementation": {"linkSupport": true},
                    "typeDefinition": {"linkSupport": true},
                    "typeHierarchy": {},
                    "inlayHint": {},
                    "codeLens": {},
                    "documentLink": {"tooltipSupport": false},
                    "references": {},
                    "documentHighlight": {},
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                    "formatting": {},
                    "rangeFormatting": {},
                    "rename": {"prepareSupport": true},
                    "codeAction": {
                        "codeActionLiteralSupport": {"codeActionKind": {"valueSet": [
                            "", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite",
                            "source", "source.organizeImports", "source.fixAll",
                        ]}},
                        "isPreferredSupport": true,
                        "disabledSupport": true,
                        "dataSupport": true,
                        "resolveSupport": {"properties": ["edit"]},
                    },
                    "diagnostic": {"dynamicRegistration": false, "relatedDocumentSupport": true},
                    "publishDiagnostics": {"relatedInformation": false, "versionSupport": true, "tagSupport": {"valueSet": [1, 2]}},
                },
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "didChangeConfiguration": {"dynamicRegistration": true},
                    "didChangeWatchedFiles": {"dynamicRegistration": true, "relativePatternSupport": false},
                    "applyEdit": true,
                    "workspaceEdit": {"documentChanges": true, "resourceOperations": ["rename"]},
                },
                "window": {"workDoneProgress": true, "showDocument": {"support": false}},
            },
        })
    }

    /// The `initialize` answer: what the server can do; it's ready.
    pub(super) fn initialized(&mut self, result: Result<Value, ResponseError>) {
        let caps = match result {
            Ok(value) => value.get("capabilities").cloned().unwrap_or(Value::Null),
            Err(e) => {
                self.status.error(format!("initialize failed: {}", e.message));
                self.exited();
                return;
            }
        };
        let provides =
            |key: &str| caps.get(key).is_some_and(|v| !v.is_null() && v != &Value::Bool(false));
        let chars = |v: Option<&Value>| -> String {
            v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect()
        };
        let completion = caps.get("completionProvider");
        let rename = caps.get("renameProvider");
        let action = caps.get("codeActionProvider");
        self.capabilities = Capabilities {
            completion: provides("completionProvider"),
            resolve: completion
                .and_then(|c| c.get("resolveProvider"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            hover: provides("hoverProvider"),
            pull_diagnostics: provides("diagnosticProvider"),
            signature_help: provides("signatureHelpProvider"),
            definition: provides("definitionProvider"),
            references: provides("referencesProvider"),
            rename: provides("renameProvider"),
            prepare_rename: rename
                .and_then(|r| r.get("prepareProvider"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            highlights: provides("documentHighlightProvider"),
            symbols: provides("documentSymbolProvider"),
            formatting: provides("documentFormattingProvider"),
            range_formatting: provides("documentRangeFormattingProvider"),
            code_actions: provides("codeActionProvider"),
            code_action_resolve: action
                .and_then(|a| a.get("resolveProvider"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            implementation: provides("implementationProvider"),
            type_definition: provides("typeDefinitionProvider"),
            type_hierarchy: provides("typeHierarchyProvider"),
            inlay_hints: provides("inlayHintProvider"),
            code_lens: provides("codeLensProvider"),
            code_lens_resolve: caps
                .get("codeLensProvider")
                .and_then(|c| c.get("resolveProvider"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            document_links: provides("documentLinkProvider"),
            completion_triggers: chars(completion.and_then(|c| c.get("triggerCharacters"))),
            signature_triggers: chars(
                caps.get("signatureHelpProvider").and_then(|s| s.get("triggerCharacters")),
            ),
        };
        self.encoding = Encoding::from_name(caps.get("positionEncoding").and_then(Value::as_str));
        let sync = caps.get("textDocumentSync");
        let kind = match sync {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::Object(o)) => o.get("change").and_then(Value::as_u64),
            _ => Some(1),
        };
        self.sync = match kind {
            Some(0) => SyncKind::None,
            Some(2) => SyncKind::Incremental,
            _ => SyncKind::Full,
        };
        self.save = sync
            .and_then(|s| s.get("save"))
            .is_some_and(|s| !s.is_null() && s != &Value::Bool(false));
        self.status.set_capabilities(self.capabilities.clone());
        if let Some(instance) = &mut self.instance {
            instance.initializing = None;
        }
        self.notify("initialized", json!({}));
        for method in self.config.on_initialized.clone() {
            self.notify(&method, Value::Null);
        }
        if !self.config.settings.is_null() {
            let settings = self.config.settings.clone();
            self.notify("workspace/didChangeConfiguration", json!({"settings": settings}));
        }
        let docs: Vec<DocumentId> = self.docs.keys().copied().collect();
        for doc in docs {
            self.did_open(doc);
        }
        self.status.log(format!("ready ({})", self.encoding.name()));
        self.set_ready_state();
    }

    pub(super) fn set_ready_state(&mut self) {
        self.status.set_documents(self.docs.len());
        if self.ready() {
            self.status.set_state(if self.docs.is_empty() { State::Idle } else { State::Ready });
            if self.docs.is_empty() {
                self.idle_since.get_or_insert(Instant::now());
            } else {
                self.idle_since = None;
            }
        }
        self.event(Event::Changed);
    }

    /// The process ended without being asked to: a crash.
    pub(super) fn exited(&mut self) {
        let was_ready = self.ready();
        self.instance = None;
        self.drop_pending();
        self.clear_diagnostics();
        if self.finished {
            return;
        }
        let now = Instant::now();
        self.status.crashed();
        self.status.set_process(None, None);
        let window = self.config.restart.window;
        // Steady for a while since its last crash: its rests start over.
        if self.last_crash.is_some_and(|t| now.duration_since(t) >= window) {
            self.rests = 0;
        }
        self.last_crash = Some(now);
        self.crashes.retain(|t| now.duration_since(*t) < window);
        self.crashes.push(now);
        let what = if was_ready { "crashed" } else { "it ended before it was ready" };
        self.status.log(what);
        tracing::warn!(server = %self.config.name, %what, "the language server stopped");
        if self.crashes.len() >= self.config.restart.crashes {
            // Rests, then tried again (never given up for good); restarted by hand at once.
            let rest = self
                .config
                .restart
                .cooldown
                .saturating_mul(2u32.saturating_pow(self.rests))
                .min(MAX_COOLDOWN.max(self.config.restart.cooldown));
            self.rests = self.rests.saturating_add(1);
            self.crashes.clear();
            self.restart_at = Some(now + rest);
            self.status.error(format!(
                "crashed {} times in {} s: stopped, tried again in {} s",
                self.config.restart.crashes,
                window.as_secs(),
                rest.as_secs()
            ));
            tracing::warn!(server = %self.config.name, ?rest, "crashed too often: tried again after a rest");
            self.status.set_state(State::Stopped { crashed: true });
        } else {
            let pause =
                self.config.restart.pause * 2u32.saturating_pow(self.crashes.len() as u32 - 1);
            self.restart_at = Some(now + pause);
            self.status.set_state(State::Restarting);
        }
        self.event(Event::Changed);
    }

    /// Stops the running instance: `shutdown`, `exit`, then its process.
    pub(super) fn stop_instance(&mut self) {
        if self.ready() {
            let id = self.next();
            self.write(&Message::Request {
                id: Id::Number(id),
                method: "shutdown".into(),
                params: Value::Null,
            });
            let until = Instant::now() + self.config.timeouts.shutdown;
            self.wait_for(until, |input, generation| match input {
                Input::Incoming(g, Message::Response { id: Id::Number(answered), .. }) => {
                    *g == generation && *answered == id
                }
                Input::Exited(g) => *g == generation,
                _ => false,
            });
            self.write(&Message::Notification { method: "exit".into(), params: Value::Null });
            let until = Instant::now() + self.config.timeouts.shutdown / 2;
            self.wait_for(
                until,
                |input, generation| matches!(input, Input::Exited(g) if *g == generation),
            );
        }
        // Its process goes with it (stopped with what it started).
        self.instance = None;
        // What an old instance still says is ignored.
        self.generation += 1;
        self.drop_pending();
        self.clear_diagnostics();
        self.status.set_process(None, None);
    }

    /// Waits until `until` for an input `done` says ends it (the others are dropped, but the
    /// documents' changes are kept).
    pub(super) fn wait_for(&mut self, until: Instant, done: impl Fn(&Input, u64) -> bool) {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            match self.rx.recv_timeout(left) {
                Ok(input) if done(&input, self.generation) => return,
                Ok(Input::Change { doc, version, text }) => {
                    if let Some(d) = self.docs.get_mut(&doc) {
                        d.pending = Some((version, text));
                    }
                }
                Ok(Input::Stop) => self.finished = true,
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.finished = true;
        self.stop_instance();
        self.status.set_state(State::Stopped { crashed: false });
        self.status.log("stopped");
    }
}
