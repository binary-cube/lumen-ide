//! The Intelligence port answered by the servers ([`LanguageServers`]): a question about a
//! document goes to the server it's open in, if that one is ready and answers it; otherwise
//! `None` (the built-in engine answers).

use std::ops::Range;
use std::sync::Arc;

use code_intelligence::hierarchy::{HierarchyNode, TypeHierarchy};
use code_intelligence::inlay::InlayHint;
use code_intelligence::links::DocumentLink;
use code_intelligence::servers::{
    CodeAction, Engine, Kept, LanguageServers, Reply, ServedCompletion, ServedLens, ServedLocation,
    ServerAt, WorkspaceChange,
};
use code_intelligence::{CompletionItem, HoverInfo, SignatureHelp, Symbol};
use kernel::ProjectId;
use lsp_client::client::{Client, FormatOptions, HierarchyItem};
use lsp_client::status::Feature;

use crate::projects;

/// The servers, as the app asks them.
#[derive(Debug, Default)]
pub struct Servers;

/// The client answering `feature` for `at`'s document now.
fn client(at: &ServerAt<'_>, feature: Feature) -> Option<Arc<Client>> {
    let project = projects::project(at.project)?;
    let client = project.client_of(at.document)?;
    client.status().answers(feature).then_some(client)
}

impl LanguageServers for Servers {
    fn served(&self, project: ProjectId, language: &str) -> Engine {
        projects::project(project).map_or(Engine::BuiltIn, |p| p.served(language))
    }

    fn kept(&self, project: ProjectId, language: &str) -> Kept {
        projects::project(project).map_or_else(Kept::default, |p| p.kept(language))
    }

    fn languages_with(&self, project: ProjectId, language: &str) -> Vec<String> {
        projects::project(project)
            .map_or_else(|| vec![language.to_owned()], |p| p.languages_with(language))
    }

    fn engine_label(&self, project: ProjectId, language: &str) -> Option<String> {
        crate::engines::engine_label(project, language)
    }

    fn completion_triggers(&self, project: ProjectId, language: &str) -> String {
        let Some(project) = projects::project(project) else { return String::new() };
        project.servers(|servers| {
            servers
                .iter()
                .filter(|r| r.languages.contains(language))
                .map(|r| r.client.status().capabilities().completion_triggers)
                .collect()
        })
    }

    fn complete(&self, at: &ServerAt<'_>) -> Option<Reply<ServedCompletion>> {
        Some(client(at, Feature::Completion)?.completion(at.document, at.offset))
    }

    fn resolve(&self, at: &ServerAt<'_>, item: &CompletionItem) -> Option<Reply<CompletionItem>> {
        item.resolve?;
        Some(client(at, Feature::Completion)?.resolve(at.document, item.clone()))
    }

    fn hover(&self, at: &ServerAt<'_>) -> Option<Reply<Option<HoverInfo>>> {
        Some(client(at, Feature::Hover)?.hover(at.document, at.offset, at.offset..at.offset))
    }

    fn signature_help(&self, at: &ServerAt<'_>) -> Option<Reply<Option<SignatureHelp>>> {
        Some(client(at, Feature::SignatureHelp)?.signature_help(at.document, at.offset))
    }

    fn definition(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        Some(client(at, Feature::Definition)?.definition(at.document, at.offset))
    }

    fn references(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        Some(client(at, Feature::References)?.references(at.document, at.offset))
    }

    fn rename_places(
        &self,
        at: &ServerAt<'_>,
    ) -> Option<Reply<Result<Vec<ServedLocation>, String>>> {
        Some(client(at, Feature::Rename)?.rename_places(at.document, at.offset))
    }

    fn highlights(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<Range<usize>>>> {
        Some(client(at, Feature::Highlights)?.highlights(at.document, at.offset))
    }

    fn symbols(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<Symbol>>> {
        Some(client(at, Feature::Symbols)?.symbols(at.document))
    }

    fn format(
        &self,
        at: &ServerAt<'_>,
        range: Option<Range<usize>>,
    ) -> Option<Reply<Result<WorkspaceChange, String>>> {
        let options = FormatOptions { tab_size: 4, insert_spaces: true };
        Some(client(at, Feature::Formatting)?.format(at.document, range, options))
    }

    fn code_actions(
        &self,
        at: &ServerAt<'_>,
        range: Range<usize>,
    ) -> Option<Reply<Vec<CodeAction>>> {
        Some(client(at, Feature::CodeActions)?.code_actions(at.document, range))
    }

    fn run_action(
        &self,
        at: &ServerAt<'_>,
        id: u64,
    ) -> Option<Reply<Result<WorkspaceChange, String>>> {
        Some(client(at, Feature::CodeActions)?.run_action(id))
    }

    fn implementations(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        Some(client(at, Feature::Implementation)?.implementations(at.document, at.offset))
    }

    fn type_definition(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        Some(client(at, Feature::TypeDefinition)?.type_definition(at.document, at.offset))
    }

    fn type_hierarchy(&self, at: &ServerAt<'_>) -> Option<Reply<Option<TypeHierarchy>>> {
        let client = client(at, Feature::TypeHierarchy)?;
        let prepared = client.prepare_type_hierarchy(at.document, at.offset);
        // Its supertypes and subtypes are asked one level after another, on a thread of
        // ours (never the app's), each waited for a little.
        Some(chained(move || {
            // Nothing there: said (the built-in engine's then); no answer: gone.
            let Some(item) = prepared.wait(STEP)? else { return Some(None) };
            let mut budget = MAX_TYPES;
            let supertypes = walk(&client, &item, true, 1, &mut budget);
            let subtypes = walk(&client, &item, false, 1, &mut budget);
            Some(Some(TypeHierarchy { focus: item.place, supertypes, subtypes }))
        }))
    }

    fn inlay_hints(&self, at: &ServerAt<'_>, range: Range<usize>) -> Option<Reply<Vec<InlayHint>>> {
        Some(client(at, Feature::InlayHints)?.inlay_hints(at.document, range))
    }

    fn code_lens(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLens>>> {
        let client = client(at, Feature::CodeLens)?;
        let listed = client.code_lens(at.document);
        // The lenses without their title yet worked out one by one (at most a screenful).
        Some(chained(move || {
            let items = listed.wait(STEP)?;
            let mut out = Vec::new();
            for item in items.into_iter().take(MAX_LENSES) {
                match &item.lens {
                    Some(lens) => out.push(lens.clone()),
                    None => {
                        if let Some(Some(lens)) = client.resolve_lens(&item).wait(STEP) {
                            out.push(lens);
                        }
                    }
                }
            }
            Some(out)
        }))
    }

    fn document_links(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<DocumentLink>>> {
        Some(client(at, Feature::DocumentLinks)?.document_links(at.document))
    }
}

/// How long one step of a chain of requests is waited for.
const STEP: std::time::Duration = std::time::Duration::from_secs(5);
/// At most this many types in a server's hierarchy, lenses in a document.
const MAX_TYPES: usize = 120;
const MAX_LENSES: usize = 200;
/// How many levels of a server's hierarchy are asked for.
const MAX_DEPTH: usize = 4;

/// A reply worked out by `work` on the module's pool (`None`: nothing to answer — the
/// reply is gone).
fn chained<T: Send + 'static>(work: impl FnOnce() -> Option<T> + Send + 'static) -> Reply<T> {
    let (tx, reply) = Reply::pair();
    kernel::work::run("lsp-chain", move || {
        if let Some(value) = work() {
            tx.send(value);
        }
    });
    reply
}

/// The server's supertypes (`up`) or subtypes of `item`, as trees.
fn walk(
    client: &Client,
    item: &HierarchyItem,
    up: bool,
    depth: usize,
    budget: &mut usize,
) -> Vec<HierarchyNode> {
    let Some(items) = client.hierarchy_step(item, up).wait(STEP) else { return Vec::new() };
    let mut out = Vec::new();
    for next in items {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let children =
            if depth < MAX_DEPTH { walk(client, &next, up, depth + 1, budget) } else { Vec::new() };
        out.push(HierarchyNode { place: next.place, children });
    }
    out
}
