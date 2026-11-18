//! A console's completion, definitions and hover cards from its source's schema (plans
//! 39j–39k), as a
//! [`code_intelligence::DocumentAssist`]: the statement at the cursor read for what's in
//! scope ([`database::sql::scope`]), the source's index asked for ([`crate::schema`]: built
//! in the background the first time, nothing waited for), the suggestions worked out by the
//! domain ([`database::completion`]) and shown with the source's name beside its tables. The
//! columns of a huge schema's tables are asked for as a statement names them; an open list
//! is made again when they come (the assist's generation).
//!
//! "Go to Definition" (⌘B, ⌘-click) on a table opens its data, on a column its structure
//! with the column pointed at (through the explorer: [`GOTO`]), on an alias, a CTE or a
//! derived table (and their columns) the place they're defined in the text; the hover card
//! of a table lists its columns with their types and keys, a column's says its type,
//! nullability, default and what it points to ([`database::navigation`]).
//!
//! A document is a console's by its file (`consoles/<source>/<name>.sql`), or a `.sql` file
//! of the project attached to a source (39n): both asked the same way, through
//! [`attachment`].

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use code_intelligence::{
    AssistContext, AssistTarget, CompletionItem, CompletionKind, DocumentAssist,
    FrameworkCompletion, HoverInfo, HoverParam,
};
use database::DataSourceId;
use database::completion::{self, CompletionRequest, Suggestion, SuggestionKind};
use database::console::{ConsoleAccess, SourceBrief};
use database::navigation::{self, LocalKind, Lookup, Target};
use database::schema_index::SchemaIndex;
use database::sql::{Expect, Flavor, Token, lex, scope_in};
use kernel::ProjectId;

use crate::consoles::{Consoles, consoles};

/// What a document runs on: its source, database and schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub source: DataSourceId,
    /// Another database of the server (`None`: the source's).
    pub database: Option<String>,
    /// The schema chosen (`None`: the connection's).
    pub schema: Option<String>,
}

/// The source `path` is a console of, or is attached to (in project `project`), with its
/// consoles.
pub fn attachment(project: ProjectId, path: &Path) -> Option<(Arc<Consoles>, Attachment)> {
    let consoles = consoles(project)?;
    // Only a console's file or a project's SQL file (a cheap look at the path first).
    let root = consoles.root();
    if database::console::identify(root, path).is_none()
        && database::console::attachable(root, path).is_none()
    {
        return None;
    }
    let spec = consoles.spec_of_path(path)?;
    let attachment =
        Attachment { source: spec.source, database: spec.database.clone(), schema: spec.schema };
    Some((consoles, attachment))
}

/// The dialect of a console's file or an attached one (its source's): how it's laid out.
pub fn flavor_of(project: ProjectId, path: &Path) -> Option<Flavor> {
    let (consoles, attachment) = attachment(project, path)?;
    Some(consoles.access()?.source(attachment.source)?.flavor)
}

/// The console's completion (see the module's doc). A document's tokens are read once
/// per version (asked again for the same text, they're reused).
#[derive(Default)]
pub struct ConsoleAssist {
    tokens: std::sync::Mutex<kernel::Memo<TokensKey, Arc<[Token]>>>,
}

/// What a document's tokens were read for: its path, its version, its dialect.
type TokensKey = (std::path::PathBuf, u64, Flavor);

impl std::fmt::Debug for ConsoleAssist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleAssist").finish_non_exhaustive()
    }
}

impl ConsoleAssist {
    /// `at`'s document's tokens, by `flavor`'s rules (read again only for a new version).
    fn tokens(&self, at: &AssistContext<'_>, flavor: Flavor) -> Arc<[Token]> {
        let mut memo = self.tokens.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (at.path.to_path_buf(), at.version, flavor);
        Arc::clone(memo.get_or_update(key, || lex(at.text, flavor).into()))
    }
}

impl DocumentAssist for ConsoleAssist {
    fn id(&self) -> &'static str {
        "database-console"
    }

    fn languages(&self) -> &'static [&'static str] {
        &["sql"]
    }

    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        let (consoles, attachment) = attachment(at.project?, at.path)?;
        let access = consoles.access()?;
        let brief = access.source(attachment.source)?;
        if !brief.dialect {
            return None;
        }
        let tokens = self.tokens(at, brief.flavor);
        let place = Place { text: at.text, tokens: Some(&tokens), offset: at.offset };
        Some(suggest(&consoles, access.as_ref(), &brief, &attachment, &place, Instant::now()))
    }

    fn definition(&self, at: &AssistContext<'_>) -> Option<AssistTarget> {
        let (consoles, attachment) = attachment(at.project?, at.path)?;
        let access = consoles.access()?;
        let brief = access.source(attachment.source)?;
        if !brief.dialect {
            return None;
        }
        let tokens = self.tokens(at, brief.flavor);
        let place = Place { text: at.text, tokens: Some(&tokens), offset: at.offset };
        let (found, _) = look(&consoles, access.as_ref(), &brief, &attachment, &place, false);
        let found = found?;
        let location = |range: std::ops::Range<usize>| {
            let line = at.text[..range.start.min(at.text.len())].matches('\n').count() as u32;
            AssistTarget::Location(code_intelligence::Location {
                path: at.path.to_path_buf(),
                offset: range.start,
                line,
            })
        };
        match found.target {
            // A use of an alias: where it's given; the table itself: its data.
            Target::Table { alias: Some(alias), .. } if alias != found.range => {
                Some(location(alias))
            }
            Target::Table { .. } | Target::Column { .. } => {
                Some(AssistTarget::Command(GOTO.0.to_owned()))
            }
            Target::Local { defined, .. } | Target::LocalColumn { defined, .. }
                if defined != found.range =>
            {
                Some(location(defined))
            }
            Target::Local { .. } | Target::LocalColumn { .. } | Target::Ambiguous { .. } => None,
        }
    }

    fn hover(&self, at: &AssistContext<'_>) -> Option<HoverInfo> {
        let (consoles, attachment) = attachment(at.project?, at.path)?;
        let access = consoles.access()?;
        let brief = access.source(attachment.source)?;
        if !brief.dialect {
            return None;
        }
        let tokens = self.tokens(at, brief.flavor);
        let place = Place { text: at.text, tokens: Some(&tokens), offset: at.offset };
        let (found, index) = look(&consoles, access.as_ref(), &brief, &attachment, &place, true);
        let found = found?;
        Some(card(&found, index.as_deref(), &brief.name))
    }

    /// A function's signature: the source's own or its engine's (the schema's routines),
    /// else one known for its dialect (48c).
    fn signature(&self, at: &AssistContext<'_>) -> Option<String> {
        let (consoles, attachment) = attachment(at.project?, at.path)?;
        let access = consoles.access()?;
        let brief = access.source(attachment.source)?;
        let end = at.offset.min(at.text.len());
        let name = at.text[..end]
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .next()
            .filter(|n| !n.is_empty())?;
        // The index as it is (asked for in the background when it isn't read yet).
        let database = attachment.database.as_deref();
        let index =
            consoles.indexes().index(access.as_ref(), attachment.source, database, Instant::now());
        completion::signature(name, brief.flavor, index.as_deref())
    }

    fn generation(&self, project: Option<ProjectId>) -> u64 {
        project.and_then(consoles).map_or(0, |c| c.indexes().generation())
    }
}

/// "Go to Definition" on a table or a column of the console's source (internal): its data
/// opened, or its structure with the column pointed at.
pub const GOTO: command_model::CommandId = command_model::CommandId("database.console.goto");

/// What the name at `place` is, with the index it was found in (asked for when there's
/// none, the columns it needs asked for when `fetch`).
fn look(
    consoles: &Arc<Consoles>,
    access: &dyn ConsoleAccess,
    brief: &SourceBrief,
    attachment: &Attachment,
    place: &Place<'_>,
    fetch: bool,
) -> (Option<Lookup>, Option<Arc<SchemaIndex>>) {
    let read;
    let tokens = match place.tokens {
        Some(tokens) => tokens,
        None => {
            read = lex(place.text, brief.flavor);
            &read[..]
        }
    };
    let indexes = consoles.indexes();
    let database = attachment.database.as_deref();
    let index = indexes.index(access, attachment.source, database, Instant::now());
    if index.is_none() {
        crate::consoles::watch();
    }
    let (found, missing) =
        navigation::lookup(place.text, tokens, place.offset, brief.flavor, index.as_deref());
    if fetch && !missing.is_empty() {
        indexes.request(attachment.source, database, &missing);
    }
    (found, index)
}

/// The hover card of what `found` is (`source`: the console's source's name).
fn card(found: &Lookup, index: Option<&SchemaIndex>, source: &str) -> HoverInfo {
    let base = HoverInfo { range: found.range.clone(), ..HoverInfo::default() };
    let place_of = |index: &SchemaIndex, i: usize| match index.schema_of(i) {
        Some(schema) => format!("{schema} · {source}"),
        None => source.to_owned(),
    };
    match &found.target {
        Target::Table { table, .. } => {
            let Some(index) = index else { return base };
            let t = index.table(*table);
            let view = t.kind == database::TableKind::View;
            let mut doc = Vec::new();
            if let Some(rows) = t.rows {
                doc.push(i18n::tr!("console.hover_rows", count = rows));
            }
            if let Some(comment) = &t.comment {
                doc.push(comment.clone());
            }
            let params = match &t.columns {
                Some(columns) => columns
                    .iter()
                    .take(HOVER_COLUMNS)
                    .map(|c| HoverParam {
                        ty: Some(c.type_name.to_string()),
                        name: c.name.to_string(),
                        doc: column_notes(index, *table, c),
                    })
                    .chain((columns.len() > HOVER_COLUMNS).then(|| HoverParam {
                        name: i18n::tr!(
                            "console.hover_more",
                            count = columns.len() - HOVER_COLUMNS
                        ),
                        ..HoverParam::default()
                    }))
                    .collect(),
                None => {
                    doc.push(i18n::t("console.completion_loading_columns"));
                    Vec::new()
                }
            };
            HoverInfo {
                code: format!("{} {}", if view { "VIEW" } else { "TABLE" }, t.table.name),
                title: Some((
                    t.table.name.clone(),
                    Some(if view { CompletionKind::View } else { CompletionKind::Table }),
                )),
                subtitle: Some(place_of(index, *table)),
                doc: (!doc.is_empty()).then(|| doc.join(" · ")),
                params,
                params_label: Some(i18n::t("console.hover_columns")),
                ..base
            }
        }
        Target::Column { table, column } => {
            let Some(index) = index else { return base };
            let t = index.table(*table);
            let Some(c) = t.columns.as_ref().and_then(|c| c.get(*column)) else { return base };
            let mut line = c.type_name.to_string();
            if c.key_position.is_some() {
                line.push_str(" PRIMARY KEY");
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = &c.default {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            let mut doc = Vec::new();
            if let Some((to, column)) = index.points_to(*table, &c.name) {
                let target = format!("{}.{column}", index.table(to).table.name);
                doc.push(i18n::tr!("console.hover_references", target = target.as_str()));
            }
            if let Some(comment) = &c.comment {
                doc.push(comment.to_string());
            }
            HoverInfo {
                code: format!("{}.{}\n{line}", t.table.name, c.name),
                title: Some((t.table.name.clone(), Some(CompletionKind::Table))),
                subtitle: Some(place_of(index, *table)),
                doc: (!doc.is_empty()).then(|| doc.join("\n")),
                ..base
            }
        }
        Target::Local { name, kind, columns, .. } => HoverInfo {
            code: name.clone(),
            title: Some((name.clone(), Some(CompletionKind::Table))),
            subtitle: Some(local_kind(*kind)),
            params: columns
                .iter()
                .take(HOVER_COLUMNS)
                .map(|c| HoverParam { name: c.clone(), ..HoverParam::default() })
                .collect(),
            params_label: Some(i18n::t("console.hover_columns")),
            ..base
        },
        Target::LocalColumn { column, owner, kind, .. } => HoverInfo {
            code: if owner.is_empty() { column.clone() } else { format!("{owner}.{column}") },
            title: Some((owner.clone(), Some(CompletionKind::Table))),
            subtitle: Some(local_kind(*kind)),
            ..base
        },
        Target::Ambiguous { column, owners } => HoverInfo {
            code: column.clone(),
            doc: Some(i18n::tr!(
                "console.check_ambiguous",
                column = column.as_str(),
                tables = owners.join(", ").as_str()
            )),
            ..base
        },
    }
}

/// The most columns a table's hover card lists.
const HOVER_COLUMNS: usize = 40;

/// What a table's card says of column `c` beside its type: its key, `NOT NULL`, its
/// default, what it points to, its comment.
fn column_notes(
    index: &SchemaIndex,
    table: usize,
    c: &database::schema_index::IndexColumn,
) -> Option<String> {
    let mut notes = Vec::new();
    if c.key_position.is_some() {
        notes.push("PK".to_owned());
    }
    if !c.nullable {
        notes.push("NOT NULL".to_owned());
    }
    if let Some(default) = &c.default {
        notes.push(format!("DEFAULT {default}"));
    }
    if let Some((to, column)) = index.points_to(table, &c.name) {
        notes.push(format!("→ {}.{column}", index.table(to).table.name));
    }
    if let Some(comment) = &c.comment {
        notes.push(comment.to_string());
    }
    (!notes.is_empty()).then(|| notes.join(" · "))
}

fn local_kind(kind: LocalKind) -> String {
    i18n::t(match kind {
        LocalKind::Cte => "console.completion_cte",
        LocalKind::Derived => "console.hover_derived",
        LocalKind::Function => "console.hover_function",
    })
}

/// "Go to Definition" on a table or a column of the console's source (see [`GOTO`]).
pub fn goto(host: &mut dyn workbench_model::WorkbenchHost) -> bool {
    let Some(project) = host.project_id() else { return false };
    let Some(file) = host.active_file() else { return false };
    let Some((consoles, attachment)) = attachment(project, &file.path) else { return false };
    let Some(access) = consoles.access() else { return false };
    let Some(brief) = access.source(attachment.source) else { return false };
    let place = Place { text: &file.text, tokens: None, offset: file.cursor };
    let (found, index) = look(&consoles, access.as_ref(), &brief, &attachment, &place, false);
    let (Some(found), Some(index)) = (found, index) else { return false };
    let opening = match found.target {
        Target::Table { table, .. } => {
            access.open_table(attachment.source, &index.table(table).table)
        }
        Target::Column { table, column } => {
            let t = index.table(table);
            let name = t.columns.as_ref().and_then(|c| c.get(column)).map(|c| c.name.to_string());
            access.open_column(attachment.source, &t.table, name.as_deref().unwrap_or_default())
        }
        _ => None,
    };
    match opening {
        Some(database::diagram::Opening::File(path, asked)) => host.open_file_with(&path, &asked),
        Some(database::diagram::Opening::View(v)) => {
            host.open_view(&v.title, v.extension, &*v.data as &dyn std::any::Any)
        }
        None => false,
    }
}

/// Where completion is asked: the text, its tokens when read already, the cursor.
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    pub text: &'a str,
    pub tokens: Option<&'a [Token]>,
    pub offset: usize,
}

/// The suggestions at `place`, in a console of `brief`'s source (`attachment`).
pub fn suggest(
    consoles: &Arc<Consoles>,
    access: &dyn ConsoleAccess,
    brief: &SourceBrief,
    attachment: &Attachment,
    place: &Place<'_>,
    now: Instant,
) -> FrameworkCompletion {
    let text = place.text;
    let (found, replace, pending) = work(consoles, access, brief, attachment, place, now);
    let mut items: Vec<CompletionItem> = found.items.iter().map(|s| item(s, &brief.name)).collect();
    // The schema not read yet: said, the list made again when it comes.
    if let Some(label) = pending {
        items.push(CompletionItem {
            label,
            kind: CompletionKind::Value,
            insert: text.get(replace.clone()).unwrap_or_default().to_owned(),
            ..CompletionItem::default()
        });
    }
    // Shown as given (best first).
    let count = items.len() as i32;
    for (i, item) in items.iter_mut().enumerate() {
        item.score = count - i as i32;
    }
    FrameworkCompletion { replace: found.replace.unwrap_or(replace), items, merge: false }
}

/// The domain's suggestions at `offset`, the word they replace, and what to say when the
/// schema isn't read yet.
fn work(
    consoles: &Arc<Consoles>,
    access: &dyn ConsoleAccess,
    brief: &SourceBrief,
    attachment: &Attachment,
    place: &Place<'_>,
    now: Instant,
) -> (completion::Completion, std::ops::Range<usize>, Option<String>) {
    let flavor = brief.flavor;
    let text = place.text;
    let read;
    let tokens = match place.tokens {
        Some(tokens) => tokens,
        None => {
            read = lex(text, flavor);
            &read[..]
        }
    };
    let scope = scope_in(text, tokens, place.offset, flavor);
    let indexes = consoles.indexes();
    let database = attachment.database.as_deref();
    let index = match scope.expect {
        Expect::Nothing if scope.star.is_none() => None,
        _ => {
            let index = indexes.index(access, attachment.source, database, now);
            crate::consoles::watch();
            index
        }
    };
    let schema = attachment.schema.as_deref().or(index.as_ref().and_then(|i| i.default_schema()));
    let found = completion::complete(&CompletionRequest {
        scope: &scope,
        index: index.as_deref(),
        flavor,
        schema,
        limit: completion::LIMIT,
    });
    // The columns this statement needs: asked for now, the list made again when they come.
    if let Some(index) = &index
        && !index.is_complete()
    {
        let mut wanted = found.missing.clone();
        for t in index.missing(&scope.named) {
            if !wanted.contains(&t) {
                wanted.push(t);
            }
        }
        if !wanted.is_empty() {
            indexes.request(attachment.source, database, &wanted);
        }
    }
    let wants_schema = !matches!(scope.expect, Expect::Nothing | Expect::Keyword(_));
    let pending = (index.is_none() && wants_schema).then(|| {
        match indexes.error(attachment.source, database) {
            Some(why) => i18n::tr!("console.completion_failed", why = why.as_str()),
            None => i18n::tr!("console.completion_reading", name = brief.name.as_str()),
        }
    });
    (found, scope.replace, pending)
}

/// A suggestion as the editor's list shows it.
fn item(s: &Suggestion, source: &str) -> CompletionItem {
    let kind = match s.kind {
        SuggestionKind::Keyword => CompletionKind::Keyword,
        SuggestionKind::Schema => CompletionKind::Schema,
        SuggestionKind::Table | SuggestionKind::Cte => CompletionKind::Table,
        SuggestionKind::View => CompletionKind::View,
        SuggestionKind::Column => CompletionKind::Column,
        SuggestionKind::Alias => CompletionKind::Variable,
        SuggestionKind::Function => CompletionKind::Function,
        SuggestionKind::Join | SuggestionKind::Columns => CompletionKind::Snippet,
        SuggestionKind::Value | SuggestionKind::Loading => CompletionKind::Value,
    };
    let detail = match s.kind {
        // The table's schema and its source, beside its name in the list.
        SuggestionKind::Table | SuggestionKind::View | SuggestionKind::Schema => {
            Some(match s.detail.as_deref() {
                Some(schema) if s.kind != SuggestionKind::Schema => format!("{schema} · {source}"),
                _ => source.to_owned(),
            })
        }
        SuggestionKind::Cte => Some(i18n::t("console.completion_cte")),
        SuggestionKind::Join if s.inferred => Some(match &s.detail {
            Some(d) => format!("{d} · {}", i18n::t("console.completion_inferred")),
            None => i18n::t("console.completion_inferred"),
        }),
        SuggestionKind::Columns if s.expansion => Some(i18n::t("console.completion_expand")),
        SuggestionKind::Columns => Some(i18n::t("console.completion_all_columns")),
        _ => s.detail.clone(),
    };
    let label = if s.kind == SuggestionKind::Loading {
        i18n::t("console.completion_loading_columns")
    } else {
        s.label.clone()
    };
    CompletionItem {
        label,
        kind,
        detail,
        insert: s.insert.clone(),
        cursor_back: s.cursor_back,
        ..CompletionItem::default()
    }
}

/// "Expand Column List" (⌥↵ on a select list's `*`): the `*` at the cursor (or right
/// before it) replaced by the columns it means.
pub fn expand_star(host: &mut dyn workbench_model::WorkbenchHost) -> bool {
    let Some(project) = host.project_id() else { return false };
    let Some(file) = host.active_file() else { return false };
    let Some((consoles, attachment)) = attachment(project, &file.path) else { return false };
    let Some(access) = consoles.access() else { return false };
    let Some(brief) = access.source(attachment.source) else { return false };
    let text = Arc::clone(&file.text);
    for offset in [file.cursor, file.cursor + 1] {
        if offset > text.len() || !text.is_char_boundary(offset) {
            continue;
        }
        let place = Place { text: &text, tokens: None, offset };
        let (found, _, _) =
            work(&consoles, access.as_ref(), &brief, &attachment, &place, Instant::now());
        if let (Some(item), Some(range)) =
            (found.items.first().filter(|i| i.expansion), found.replace)
        {
            return host.edit_active(range, &item.insert);
        }
    }
    host.notify(workbench_model::Notification::info(i18n::t("console.expand_nothing")));
    false
}

#[cfg(test)]
#[path = "../tests/unit/assist.rs"]
mod tests;
