//! What can be asked of a server.

use super::*;

impl Client {
    /// Suggestions at byte `offset` of document `doc` (its version as last told).
    pub fn completion(&self, doc: DocumentId, offset: usize) -> Reply<ServedCompletion> {
        self.ask(
            Some(Feature::Completion),
            Some((Slot::Completion, doc)),
            move |d| {
                let (uri, position, lines) = d.at(doc, offset)?;
                let before = lines.text()[..offset.min(lines.text().len())].chars().next_back();
                let trigger = before.filter(|c| d.capabilities.completion_triggers.contains(*c));
                let context = match trigger {
                    Some(c) => json!({"triggerKind": 2, "triggerCharacter": c.to_string()}),
                    None => json!({"triggerKind": 1}),
                };
                let params = json!({"textDocument": {"uri": uri}, "position": position.to_value(), "context": context});
                Ok(Prepared { method: "textDocument/completion", params, lines: Some(lines) })
            },
            move |result, lines, d| {
                let lines = lines?;
                let value = result.ok()?;
                let kinds = &d.config.completion_kinds;
                let mut served = convert::completion_with(&value, &lines, d.encoding, offset, kinds);
                d.completion_seq += 1;
                let seq = d.completion_seq;
                // The raw items (the indexes the suggestions' handles name), with the
                // list's default data, for resolving one.
                let data = value.get("itemDefaults").and_then(|d| d.get("data")).cloned();
                let raw = match value {
                    Value::Array(items) => items,
                    Value::Object(mut list) => match list.remove("items") {
                        Some(Value::Array(items)) => items,
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                };
                let raw = raw
                    .into_iter()
                    .map(|mut item| {
                        if let (Some(data), Value::Object(o)) = (&data, &mut item) {
                            o.entry("data").or_insert_with(|| data.clone());
                        }
                        item
                    })
                    .collect();
                d.completions = (seq, if d.capabilities.resolve { raw } else { Vec::new() });
                if d.capabilities.resolve {
                    for item in &mut served.items {
                        item.item.resolve = item.item.resolve.map(|i| (seq << 20) | i);
                    }
                } else {
                    for item in &mut served.items {
                        item.item.resolve = None;
                    }
                }
                Some(served)
            },
        )
    }

    /// Suggestion `item` (of the last completion) worked out: its imports.
    pub fn resolve(&self, doc: DocumentId, item: CompletionItem) -> Reply<CompletionItem> {
        let shown = item.clone();
        self.ask(
            Some(Feature::Completion),
            None,
            move |d| {
                let handle = item.resolve.ok_or("nothing to resolve")?;
                let (seq, index) = (handle >> 20, (handle & 0xF_FFFF) as usize);
                if seq != d.completions.0 {
                    return Err("an older list".into());
                }
                let raw = d.completions.1.get(index).cloned().ok_or("no such item")?;
                let lines = d.docs.get(&doc).map(|x| x.mirror.shared());
                Ok(Prepared { method: "completionItem/resolve", params: raw, lines })
            },
            move |result, lines, d| match (result, lines) {
                (Ok(value), Some(lines)) => {
                    Some(convert::resolved(&value, &shown, &lines, d.encoding))
                }
                _ => Some(CompletionItem { resolve: None, ..shown }),
            },
        )
    }

    /// The hover card at byte `offset` (`word`: the editor's word there).
    pub fn hover(
        &self,
        doc: DocumentId,
        offset: usize,
        word: Range<usize>,
    ) -> Reply<Option<HoverInfo>> {
        self.ask(
            Some(Feature::Hover),
            Some((Slot::Hover, doc)),
            move |d| d.position_request(doc, offset, "textDocument/hover", json!({})),
            move |result, lines, d| Some(convert::hover(&result.ok()?, &*lines?, d.encoding, word)),
        )
    }

    pub fn signature_help(&self, doc: DocumentId, offset: usize) -> Reply<Option<SignatureHelp>> {
        self.ask(
            Some(Feature::SignatureHelp),
            Some((Slot::Signature, doc)),
            move |d| {
                d.position_request(
                    doc,
                    offset,
                    "textDocument/signatureHelp",
                    json!({"context": {"triggerKind": 1, "isRetrigger": false}}),
                )
            },
            move |result, _, _| Some(convert::signature_help(&result.ok()?)),
        )
    }

    pub fn definition(&self, doc: DocumentId, offset: usize) -> Reply<Vec<ServedLocation>> {
        self.ask(
            Some(Feature::Definition),
            None,
            move |d| d.position_request(doc, offset, "textDocument/definition", json!({})),
            move |result, _, d| Some(d.locations(&result.ok()?)),
        )
    }

    /// What implements the type, or overrides the method, at `offset`.
    pub fn implementations(&self, doc: DocumentId, offset: usize) -> Reply<Vec<ServedLocation>> {
        self.ask(
            Some(Feature::Implementation),
            None,
            move |d| d.position_request(doc, offset, "textDocument/implementation", json!({})),
            move |result, _, d| Some(d.locations(&result.ok()?)),
        )
    }

    /// Where the type of what's at `offset` is declared.
    pub fn type_definition(&self, doc: DocumentId, offset: usize) -> Reply<Vec<ServedLocation>> {
        self.ask(
            Some(Feature::TypeDefinition),
            None,
            move |d| d.position_request(doc, offset, "textDocument/typeDefinition", json!({})),
            move |result, _, d| Some(d.locations(&result.ok()?)),
        )
    }

    /// The type at `offset` as the server's hierarchy has it (`None`: none there), for
    /// [`Self::hierarchy_step`].
    pub fn prepare_type_hierarchy(
        &self,
        doc: DocumentId,
        offset: usize,
    ) -> Reply<Option<HierarchyItem>> {
        self.ask(
            Some(Feature::TypeHierarchy),
            None,
            move |d| {
                d.position_request(doc, offset, "textDocument/prepareTypeHierarchy", json!({}))
            },
            move |result, _, d| {
                let value = result.ok()?;
                let item = value.as_array().and_then(|a| a.first()).cloned();
                Some(item.and_then(|raw| d.hierarchy_item(raw)))
            },
        )
    }

    /// The supertypes (`up`) or subtypes of a type of the server's hierarchy.
    pub fn hierarchy_step(&self, item: &HierarchyItem, up: bool) -> Reply<Vec<HierarchyItem>> {
        let raw = item.raw.clone();
        let method = if up { "typeHierarchy/supertypes" } else { "typeHierarchy/subtypes" };
        self.ask(
            Some(Feature::TypeHierarchy),
            None,
            move |_| Ok(Prepared { method, params: json!({"item": raw}), lines: None }),
            move |result, _, d| {
                let items = result.ok()?;
                let items = items.as_array().cloned().unwrap_or_default();
                Some(items.into_iter().filter_map(|raw| d.hierarchy_item(raw)).collect())
            },
        )
    }

    /// The hints the server shows in `range` of document `doc`.
    pub fn inlay_hints(
        &self,
        doc: DocumentId,
        range: Range<usize>,
    ) -> Reply<Vec<code_intelligence::inlay::InlayHint>> {
        self.ask(
            Some(Feature::InlayHints),
            None,
            move |d| {
                let x = d.docs.get(&doc).ok_or("not open")?;
                let lines = x.mirror.shared();
                let (start, end) = lines.range(&range, d.encoding);
                let params =
                    json!({"textDocument": {"uri": x.uri}, "range": range_value(start, end)});
                Ok(Prepared { method: "textDocument/inlayHint", params, lines: Some(lines) })
            },
            move |result, lines, d| Some(convert::inlay_hints(&result.ok()?, &*lines?, d.encoding)),
        )
    }

    /// The document's lenses: the ones with their title, and those still to be worked out
    /// ([`Self::resolve_lens`]).
    pub fn code_lens(&self, doc: DocumentId) -> Reply<Vec<LensItem>> {
        self.ask(
            Some(Feature::CodeLens),
            None,
            move |d| {
                let x = d.docs.get(&doc).ok_or("not open")?;
                let params = json!({"textDocument": {"uri": x.uri}});
                Ok(Prepared {
                    method: "textDocument/codeLens",
                    params,
                    lines: Some(x.mirror.shared()),
                })
            },
            move |result, lines, d| {
                let lines = lines?;
                let value = result.ok()?;
                let resolvable = d.capabilities.code_lens_resolve;
                Some(
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|raw| match convert::lens(raw, &lines, d.encoding) {
                            Some(lens) => Some(LensItem { lens: Some(lens), raw: None, doc }),
                            None if resolvable && raw.get("range").is_some() => {
                                Some(LensItem { lens: None, raw: Some(raw.clone()), doc })
                            }
                            None => None,
                        })
                        .collect(),
                )
            },
        )
    }

    /// A lens of [`Self::code_lens`] worked out (its title).
    pub fn resolve_lens(
        &self,
        item: &LensItem,
    ) -> Reply<Option<code_intelligence::servers::ServedLens>> {
        let raw = item.raw.clone().unwrap_or_default();
        let doc = item.doc;
        self.ask(
            Some(Feature::CodeLens),
            None,
            move |d| {
                let lines = d.docs.get(&doc).map(|x| x.mirror.shared());
                Ok(Prepared { method: "codeLens/resolve", params: raw, lines })
            },
            move |result, lines, d| Some(convert::lens(&result.ok()?, &*lines?, d.encoding)),
        )
    }

    /// The links the server finds in document `doc`.
    pub fn document_links(
        &self,
        doc: DocumentId,
    ) -> Reply<Vec<code_intelligence::links::DocumentLink>> {
        self.ask(
            Some(Feature::DocumentLinks),
            None,
            move |d| {
                let x = d.docs.get(&doc).ok_or("not open")?;
                let params = json!({"textDocument": {"uri": x.uri}});
                Ok(Prepared {
                    method: "textDocument/documentLink",
                    params,
                    lines: Some(x.mirror.shared()),
                })
            },
            move |result, lines, d| {
                Some(convert::document_links(&result.ok()?, &*lines?, d.encoding))
            },
        )
    }

    /// The usages of the symbol at `offset`, its declaration included.
    pub fn references(&self, doc: DocumentId, offset: usize) -> Reply<Vec<ServedLocation>> {
        self.ask(
            Some(Feature::References),
            None,
            move |d| {
                d.position_request(
                    doc,
                    offset,
                    "textDocument/references",
                    json!({"context": {"includeDeclaration": true}}),
                )
            },
            move |result, _, d| Some(d.locations(&result.ok()?)),
        )
    }

    /// The occurrences of the symbol at `offset` in its document.
    pub fn highlights(&self, doc: DocumentId, offset: usize) -> Reply<Vec<Range<usize>>> {
        self.ask(
            Some(Feature::Highlights),
            Some((Slot::Highlights, doc)),
            move |d| d.position_request(doc, offset, "textDocument/documentHighlight", json!({})),
            move |result, lines, d| Some(convert::highlights(&result.ok()?, &*lines?, d.encoding)),
        )
    }

    pub fn symbols(&self, doc: DocumentId) -> Reply<Vec<Symbol>> {
        self.ask(
            Some(Feature::Symbols),
            None,
            move |d| {
                let x = d.docs.get(&doc).ok_or("not open")?;
                let params = json!({"textDocument": {"uri": x.uri}});
                Ok(Prepared {
                    method: "textDocument/documentSymbol",
                    params,
                    lines: Some(x.mirror.shared()),
                })
            },
            move |result, lines, d| Some(convert::symbols(&result.ok()?, &*lines?, d.encoding)),
        )
    }

    /// What renaming the symbol at `offset` to `new_name` changes.
    pub fn rename(
        &self,
        doc: DocumentId,
        offset: usize,
        new_name: String,
    ) -> Reply<Result<WorkspaceChange, String>> {
        self.ask(
            Some(Feature::Rename),
            None,
            move |d| {
                d.position_request(doc, offset, "textDocument/rename", json!({"newName": new_name}))
            },
            move |result, _, d| Some(result.map(|v| d.workspace_edit(&v))),
        )
    }

    /// The places renaming the symbol at `offset` changes (its name in each), asked as a
    /// rename to its own name (after `prepareRename`, where the server has it).
    pub fn rename_places(
        &self,
        doc: DocumentId,
        offset: usize,
    ) -> Reply<Result<Vec<ServedLocation>, String>> {
        let (tx, reply) = Reply::pair();
        let wake = Arc::clone(&self.wake);
        let token = self.tokens.fetch_add(1, Ordering::Relaxed);
        let cancel = self.tx.clone();
        let reply = reply.on_cancel(move || {
            let _ = cancel.send(Input::Cancel(token));
        });
        let answer: Answer = Box::new(move |result, lines, driver| {
            let done = move |value: Result<Vec<ServedLocation>, String>| {
                tx.send(value);
                wake();
            };
            let Some(lines) = lines else { return done(Err("not open".into())) };
            // The name as the server says it, else the word there.
            let word = code_intelligence::word_at(lines.text(), offset, "$#")
                .map(|r| lines.text()[r].to_owned())
                .unwrap_or_default();
            let name = match result {
                Err(e) => return done(Err(e)),
                Ok(Value::Null) if driver.capabilities.prepare_rename => {
                    return done(Err(String::new()));
                }
                Ok(v) => v.get("placeholder").and_then(Value::as_str).map_or(word, str::to_owned),
            };
            if name.is_empty() {
                return done(Err(String::new()));
            }
            let Ok(prepared) = driver.position_request(
                doc,
                offset,
                "textDocument/rename",
                json!({"newName": name}),
            ) else {
                return done(Err("not open".into()));
            };
            driver.request(
                prepared,
                Duration::ZERO,
                Box::new(move |result, _, d| {
                    let here = d.docs.get(&doc).map(|x| x.path.clone());
                    done(result.map(|v| {
                        let change = d.workspace_edit(&v);
                        // The name renamed is what the edit at the place replaces (`--gap`,
                        // `card-title`: more than the word there, which was sent as the
                        // new name); the places are where that is replaced by it.
                        let renamed = change
                            .files
                            .iter()
                            .filter(|f| Some(&f.path) == here.as_ref())
                            .flat_map(|f| &f.edits)
                            .find(|e| e.range.start <= offset && offset <= e.range.end)
                            .map_or_else(|| name.clone(), |e| e.original.clone());
                        change
                            .files
                            .iter()
                            .flat_map(|f| {
                                let renamed = renamed.clone();
                                let name = name.clone();
                                f.edits
                                    .iter()
                                    .filter(move |e| e.original == renamed && e.text == name)
                                    .map(move |e| ServedLocation {
                                        path: f.path.clone(),
                                        range: e.range.clone(),
                                        line: e.line,
                                    })
                            })
                            .collect()
                    }));
                }),
            );
        });
        let build: Build = Box::new(move |d| {
            if d.capabilities.prepare_rename {
                d.position_request(doc, offset, "textDocument/prepareRename", json!({}))
            } else {
                // No preparing: straight to the rename (the answer above asks it).
                let lines = d.docs.get(&doc).map(|x| x.mirror.shared());
                Ok(Prepared { method: "", params: Value::Null, lines })
            }
        });
        let ask = Ask {
            token,
            feature: Some(Feature::Rename),
            slot: None,
            build,
            answer,
            timeout: Duration::ZERO,
        };
        let _ = self.tx.send(Input::Ask(Box::new(ask)));
        reply
    }

    /// The document laid out (`range`: only that part, where the server can).
    pub fn format(
        &self,
        doc: DocumentId,
        range: Option<Range<usize>>,
        options: FormatOptions,
    ) -> Reply<Result<WorkspaceChange, String>> {
        self.ask(
            Some(Feature::Formatting),
            None,
            move |d| {
                let x = d.docs.get(&doc).ok_or("not open")?;
                let lines = x.mirror.shared();
                let options = json!({"tabSize": options.tab_size, "insertSpaces": options.insert_spaces});
                let (method, params) = match range.filter(|_| d.capabilities.range_formatting) {
                    Some(range) => {
                        let (start, end) = lines.range(&range, d.encoding);
                        ("textDocument/rangeFormatting", json!({"textDocument": {"uri": x.uri}, "range": range_value(start, end), "options": options}))
                    }
                    None if d.capabilities.formatting => ("textDocument/formatting", json!({"textDocument": {"uri": x.uri}, "options": options})),
                    None => return Err("the server can't lay out a whole file".into()),
                };
                Ok(Prepared { method, params, lines: Some(lines) })
            },
            move |result, lines, d| {
                let path = d.docs.get(&doc).map(|x| x.path.clone());
                Some(result.and_then(|value| {
                    let (lines, path) = lines.zip(path).ok_or("not open")?;
                    let file = convert::file_change(&path, &value, &lines, d.encoding);
                    Ok(WorkspaceChange { files: vec![file], renames: Vec::new() })
                }))
            },
        )
    }

    /// The fixes and refactorings offered for `range`.
    pub fn code_actions(&self, doc: DocumentId, range: Range<usize>) -> Reply<Vec<CodeAction>> {
        self.ask(
            Some(Feature::CodeActions),
            None,
            move |d| {
                let x = d.docs.get(&doc).ok_or("not open")?;
                let lines = x.mirror.shared();
                let (start, end) = lines.range(&range, d.encoding);
                // The diagnostics there: the quick fixes are for them.
                let diagnostics: Vec<Value> = d
                    .published
                    .get(&x.uri)
                    .into_iter()
                    .flatten()
                    .filter(|diagnostic| {
                        crate::text::range_from_value(&diagnostic["range"])
                            .is_some_and(|(s, e)| s <= end && start <= e)
                    })
                    .cloned()
                    .collect();
                let params = json!({
                    "textDocument": {"uri": x.uri},
                    "range": range_value(start, end),
                    "context": {"diagnostics": diagnostics, "triggerKind": 1},
                });
                Ok(Prepared { method: "textDocument/codeAction", params, lines: Some(lines) })
            },
            move |result, _, d| {
                let actions = convert::code_actions(&result.ok()?);
                d.action_seq += 1;
                let seq = d.action_seq;
                let (shown, raw): (Vec<CodeAction>, Vec<Value>) = actions
                    .into_iter()
                    .map(|(mut a, raw)| {
                        a.id |= seq << 20;
                        (a, raw)
                    })
                    .unzip();
                d.actions = (seq, raw);
                Some(shown)
            },
        )
    }

    /// Code action `id` (of the last list) worked out and run: its edit (a command the server
    /// runs itself, its edits coming as [`Event::ApplyEdit`]).
    pub fn run_action(&self, id: u64) -> Reply<Result<WorkspaceChange, String>> {
        self.ask(
            Some(Feature::CodeActions),
            None,
            move |d| {
                let (seq, index) = (id >> 20, (id & 0xF_FFFF) as usize);
                if seq != d.actions.0 {
                    return Err("an older list".into());
                }
                let raw = d.actions.1.get(index).cloned().ok_or("no such action")?;
                if raw.get("edit").is_some() || raw.get("command").is_some_and(Value::is_string) {
                    // Nothing to ask: the answer below has it.
                    return Ok(Prepared { method: "", params: raw, lines: None });
                }
                if d.capabilities.code_action_resolve {
                    return Ok(Prepared { method: "codeAction/resolve", params: raw, lines: None });
                }
                Ok(Prepared { method: "", params: raw, lines: None })
            },
            move |result, _, d| Some(result.map(|action| d.run_code_action(&action))),
        )
    }
}
