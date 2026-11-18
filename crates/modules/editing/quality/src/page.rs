//! The "Code quality" page of the settings: the open project's choices, one language at a
//! time (the languages some module has a tool for) — its formatter and its checker (none, a
//! tool its module knows, a binary, or a command / script of the project), the tool whose
//! arguments or output format a binary / command follows, what runs on save — and what
//! exactly a run would start (or why nothing can). Kept in the project's own settings.

use std::path::Path;
use std::sync::Arc;

use code_quality::plan::PlanError;
use code_quality::{Choice, Invocation, Kind, Program, QualityTool};
use i18n::{t, tr};
use settings_model::SettingsService;
use workbench_model::{
    OnYes, Question, SettingsBlock, SettingsContribution, SettingsLanguage, SettingsPage,
    SettingsRow, SettingsScope,
};

use crate::runner::ProjectRuns;
use crate::settings::{self, Saved};

/// The choices' values (a tool's: its place among the language's tools, from 1).
const NONE: i64 = 0;
const BINARY: i64 = -1;
const COMMAND: i64 = -2;

/// The block (Settings → Languages: the language chosen at the page's top).
#[derive(Debug, Default)]
pub(crate) struct QualitySettings;

/// A kind's keys in the block.
struct Keys {
    what: &'static str,
    value: &'static str,
    reads: &'static str,
    on_save: &'static str,
}

fn keys(kind: Kind) -> Keys {
    match kind {
        Kind::Formatter => Keys {
            what: "formatter",
            value: "formatter_value",
            reads: "formatter_reads",
            on_save: "format_on_save",
        },
        Kind::Checker => Keys {
            what: "checker",
            value: "checker_value",
            reads: "checker_reads",
            on_save: "check_on_save",
        },
    }
}

fn kind_of(key: &str) -> Option<Kind> {
    [Kind::Formatter, Kind::Checker].into_iter().find(|&kind| {
        let k = keys(kind);
        [k.what, k.value, k.reads, k.on_save].contains(&key)
    })
}

/// The languages some tool works on: the known ones in their order, then the others.
pub(crate) fn languages(tools: &[Arc<dyn QualityTool>]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for language in tools.iter().flat_map(|t| t.languages().iter().copied()) {
        if !out.contains(&language) {
            out.push(language);
        }
    }
    let rank = |l: &str| KNOWN.iter().position(|(id, _)| *id == l).unwrap_or(KNOWN.len());
    out.sort_by_key(|l| rank(l));
    out
}

/// The languages' names as shown, in the page's order.
const KNOWN: [(&str, &str); 10] = [
    ("php", "PHP"),
    ("javascript", "JavaScript"),
    ("typescript", "TypeScript"),
    ("tsx", "TSX"),
    ("vue", "Vue"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("json", "JSON"),
    ("yaml", "YAML"),
];

/// The `kind` tools for `language`.
fn tools_for<'a>(
    tools: &'a [Arc<dyn QualityTool>],
    kind: Kind,
    language: &'a str,
) -> impl Iterator<Item = &'a Arc<dyn QualityTool>> + 'a {
    tools.iter().filter(move |t| t.kind() == kind && t.languages().contains(&language))
}

/// A language's name as shown.
pub(crate) fn language_name(id: &str) -> String {
    KNOWN.iter().find(|(k, _)| *k == id).map_or_else(|| id.to_owned(), |(_, name)| (*name).into())
}

/// The runs of the project whose settings these are.
fn runs_of(project: &SettingsService) -> Option<Arc<ProjectRuns>> {
    crate::PROJECTS.all().into_iter().find(|runs| runs.uses(project))
}

impl QualitySettings {
    /// The page's language, when some tool works on it.
    fn language(
        &self,
        tools: &[Arc<dyn QualityTool>],
        scope: &SettingsScope<'_>,
    ) -> Option<&'static str> {
        let chosen = scope.language?;
        languages(tools).into_iter().find(|l| *l == chosen)
    }

    /// The rows of a `kind` tool for `language`.
    fn kind_rows(
        &self,
        runs: &ProjectRuns,
        project: &SettingsService,
        language: &'static str,
        kind: Kind,
    ) -> Vec<SettingsRow> {
        let k = keys(kind);
        let saved = settings::saved(project, language, kind);
        let tools: Vec<_> = tools_for(runs.tools(), kind, language).collect();
        let mut choices = vec![(NONE, t("settings.quality_none"))];
        choices.extend(tools.iter().enumerate().map(|(i, t)| (i as i64 + 1, t.name().to_owned())));
        choices.push((BINARY, t("settings.quality_binary")));
        choices.push((COMMAND, t("settings.quality_command")));
        let current = match saved.what.as_str() {
            "tool" => {
                tools.iter().position(|t| t.id() == saved.value).map_or(NONE, |i| i as i64 + 1)
            }
            "binary" => BINARY,
            "command" => COMMAND,
            _ => NONE,
        };
        let (label, on_save) = match kind {
            Kind::Formatter => ("settings.quality_formatter", "settings.quality_format_on_save"),
            Kind::Checker => ("settings.quality_checker", "settings.quality_check_on_save"),
        };
        let mut rows = vec![SettingsRow::Choices {
            key: k.what,
            label: t(label),
            detail: t(&format!("{label}_detail")),
            choices,
            current,
            default: Some(NONE),
        }];
        if current == NONE {
            // A tool the project has (its `node_modules`' ESLint, Prettier…): offered, run
            // only once the user says so.
            if let Some(found) = tools.iter().find_map(|t| in_project(runs.root(), t.as_ref())) {
                let tool = match &found.version {
                    Some(v) => format!("{} {v}", found.name),
                    None => found.name.to_owned(),
                };
                let path = found.place.as_str();
                let text = if path.starts_with("node_modules/") {
                    tr!("settings.quality_found_node", tool = tool.as_str(), path = path)
                } else {
                    tr!("settings.quality_found", tool = tool.as_str(), path = path)
                };
                rows.push(SettingsRow::Status {
                    text,
                    button: Some((use_action(kind), t("settings.quality_use"))),
                });
            }
            return rows;
        }
        if current == BINARY || current == COMMAND {
            let (label, placeholder) = if current == BINARY {
                ("settings.quality_path", "settings.quality_path_hint")
            } else {
                ("settings.quality_line", "settings.quality_line_hint")
            };
            rows.push(SettingsRow::Text {
                key: k.value,
                label: t(label),
                detail: t(&format!("{label}_detail")),
                value: saved.value.clone(),
                placeholder: t(placeholder),
            });
            // A binary takes a tool's arguments; a checker's output reads as a tool's.
            if current == BINARY || kind == Kind::Checker {
                let (label, plain) = match kind {
                    Kind::Formatter => ("settings.quality_args_as", "settings.quality_args_file"),
                    Kind::Checker => ("settings.quality_reads_as", "settings.quality_reads_lines"),
                };
                let mut choices = vec![(NONE, t(plain))];
                choices.extend(
                    tools.iter().enumerate().map(|(i, t)| (i as i64 + 1, t.name().to_owned())),
                );
                let current =
                    tools.iter().position(|t| t.id() == saved.reads).map_or(NONE, |i| i as i64 + 1);
                rows.push(SettingsRow::Choices {
                    key: k.reads,
                    label: t(label),
                    detail: t(&format!("{label}_detail")),
                    choices,
                    current,
                    default: Some(NONE),
                });
            }
        }
        // What the project's settings choose runs once the user allowed it (they may have
        // come with the project).
        let chosen = chosen(project, language, kind);
        let row = if runs.allows(&chosen) {
            SettingsRow::Status { text: status(runs, &saved, kind, language), button: None }
        } else {
            SettingsRow::Status {
                text: tr!("settings.quality_not_allowed", what = runs.shown(&chosen)),
                button: Some((allow_action(kind), t("settings.quality_allow"))),
            }
        };
        rows.push(row);
        rows.push(SettingsRow::Toggle {
            key: k.on_save,
            label: t(on_save),
            detail: t(&format!("{on_save}_detail")),
            default: None,
            on: settings::on_save(project, language, kind),
        });
        rows
    }
}

/// The project's choice of a `kind` tool for `language`.
fn chosen(project: &SettingsService, language: &str, kind: Kind) -> Choice {
    let choice = settings::choice(project, language);
    match kind {
        Kind::Formatter => choice.formatter,
        Kind::Checker => choice.checker,
    }
}

/// A tool found in the project itself (its package in `node_modules`, its `vendor/bin`
/// program): its name, where, its version when its package says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub id: &'static str,
    pub name: &'static str,
    /// Relative to the project (`node_modules/eslint`).
    pub place: String,
    pub version: Option<String>,
}

/// `tool`, if the project at `root` has it (a script or a program of it in the project).
pub(crate) fn in_project(root: &Path, tool: &dyn QualityTool) -> Option<Found> {
    let place = tool
        .scripts()
        .iter()
        .chain(tool.candidates())
        .filter(|c| c.contains('/'))
        .find(|c| root.join(c).is_file())?;
    // A package's folder (`node_modules/<name>`) and its version.
    let parts: Vec<&str> = place.split('/').collect();
    let package = match parts.as_slice() {
        ["node_modules", ".bin", name] => Some(format!("node_modules/{name}")),
        ["node_modules", name, ..] => Some(format!("node_modules/{name}")),
        _ => None,
    };
    let version = package.as_ref().and_then(|p| {
        let text = std::fs::read_to_string(root.join(p).join("package.json")).ok()?;
        let at = text.find("\"version\"")?;
        let rest = text[at + 9..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
        Some(rest[..rest.find('"')?].to_owned())
    });
    Some(Found {
        id: tool.id(),
        name: tool.name(),
        place: package.unwrap_or_else(|| (*place).to_owned()),
        version,
    })
}

/// The project's own `found` tool chosen as its `kind` tool for `language` (the user said
/// yes: it's allowed to run).
pub(crate) fn use_found(runs: &ProjectRuns, language: &str, kind: Kind, found: &Found) {
    runs.allow(&Choice::Tool(found.id.to_owned()));
    let saved = Saved { what: "tool".into(), value: found.id.into(), reads: String::new() };
    if let Some(Err(e)) = runs.settings().map(|s| settings::save(&s, language, kind, &saved)) {
        tracing::warn!("can't save a code quality choice: {e}");
    }
}

/// The status button choosing a `kind` tool the project has.
fn use_action(kind: Kind) -> &'static str {
    match kind {
        Kind::Formatter => "use-formatter",
        Kind::Checker => "use-checker",
    }
}

/// The status button allowing a `kind` tool.
fn allow_action(kind: Kind) -> &'static str {
    match kind {
        Kind::Formatter => "allow-formatter",
        Kind::Checker => "allow-checker",
    }
}

/// What a run would start, or why nothing can.
fn status(runs: &ProjectRuns, saved: &Saved, kind: Kind, language: &str) -> String {
    if saved.what == "command" {
        return match saved.value.as_str() {
            "" => t("settings.quality_line_missing"),
            line if line.contains("{file}") => tr!("settings.quality_runs_file", line = line),
            line => tr!("settings.quality_runs_project", line = line),
        };
    }
    match runs.preview(kind, language) {
        Ok(invocation) => match &invocation.program {
            Program::Exec(path) if !path.is_file() => {
                tr!("settings.quality_missing", path = shown(runs.root(), path))
            }
            _ => tr!("settings.quality_runs_file", line = command_line(runs.root(), &invocation)),
        },
        Err(PlanError::NotFound(name)) => tr!("settings.quality_not_found", tool = name),
        Err(PlanError::UnknownTool(id)) => tr!("quality.unknown_tool", tool = id),
        Err(PlanError::Nothing) => t("settings.quality_path_missing"),
        Err(PlanError::NotAllowed(what)) => tr!("settings.quality_not_allowed", what = what),
    }
}

/// A path as shown: relative to the project when it's in it.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn command_line(root: &Path, invocation: &Invocation) -> String {
    match &invocation.program {
        Program::Exec(path) => std::iter::once(shown(root, path))
            .chain(invocation.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" "),
        Program::Shell(line) => line.clone(),
    }
}

impl SettingsContribution for QualitySettings {
    fn id(&self) -> &'static str {
        "quality"
    }

    fn page(&self) -> &'static str {
        "languages"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGES)
    }

    fn order(&self) -> u16 {
        20
    }

    fn languages(&self, scope: SettingsScope<'_>) -> Vec<SettingsLanguage> {
        let Some(runs) = scope.project.and_then(runs_of) else { return Vec::new() };
        languages(runs.tools())
            .into_iter()
            .map(|l| SettingsLanguage { id: l.to_owned(), name: language_name(l) })
            .collect()
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let mut block = SettingsBlock {
            title: t("settings.quality"),
            detail: t("settings.quality_detail"),
            rows: Vec::new(),
        };
        let Some((project, runs)) = scope.project.and_then(|p| Some((p, runs_of(p)?))) else {
            block
                .rows
                .push(SettingsRow::Status { text: t("settings.quality_no_project"), button: None });
            return block;
        };
        // A language no tool works on: nothing to say.
        let Some(language) = self.language(runs.tools(), &scope) else { return block };
        for kind in [Kind::Formatter, Kind::Checker] {
            block.rows.extend(self.kind_rows(&runs, project, language, kind));
        }
        block
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let Some((project, runs)) = scope.project.and_then(|p| Some((p, runs_of(p)?))) else {
            return;
        };
        let tools = runs.tools();
        let (Some(language), Some(kind)) = (self.language(tools, &scope), kind_of(key)) else {
            return;
        };
        let k = keys(kind);
        let tool_id = |value: i64| {
            let at = usize::try_from(value - 1).ok()?;
            tools_for(tools, kind, language).nth(at).map(|t| t.id().to_owned())
        };
        let result = if key == k.on_save {
            settings::save_on_save(project, language, kind, value != 0)
        } else {
            let mut saved = settings::saved(project, language, kind);
            if key == k.what {
                let what = match value {
                    BINARY => "binary",
                    COMMAND => "command",
                    NONE => "none",
                    _ => "tool",
                };
                if what == "tool" {
                    saved = Saved {
                        what: what.into(),
                        value: tool_id(value).unwrap_or_default(),
                        reads: String::new(),
                    };
                    // Picked here: the user's choice.
                    runs.allow(&code_quality::Choice::Tool(saved.value.clone()));
                } else if saved.what != what {
                    // Another way: what was written for the other one doesn't fit it.
                    saved = Saved { what: what.into(), ..Saved::default() };
                }
            } else if key == k.reads {
                saved.reads = tool_id(value).unwrap_or_default();
            }
            settings::save(project, language, kind, &saved)
        };
        if let Err(e) = result {
            tracing::warn!("can't save a code quality choice: {e}");
        }
    }

    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let Some((project, runs)) = scope.project.and_then(|p| Some((p, runs_of(p)?))) else {
            return;
        };
        let (Some(language), Some(kind)) = (self.language(runs.tools(), &scope), kind_of(key))
        else {
            return;
        };
        if key != keys(kind).value {
            return;
        }
        let mut saved = settings::saved(project, language, kind);
        saved.value = value.trim().to_owned();
        if let Err(e) = settings::save(project, language, kind, &saved) {
            tracing::warn!("can't save a code quality choice: {e}");
            return;
        }
        // Written here: the user's choice.
        runs.allow(&chosen(project, language, kind));
    }

    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        let project = scope.project?;
        let runs = runs_of(project)?;
        let language = self.language(runs.tools(), &scope)?;
        // The project's own tool chosen: its code runs, so asked first.
        if let Some(kind) =
            [Kind::Formatter, Kind::Checker].into_iter().find(|&k| use_action(k) == action)
        {
            let found = tools_for(runs.tools(), kind, language)
                .find_map(|t| in_project(runs.root(), t.as_ref()))?;
            let question = Question {
                title: t("dialog.quality_allow_title"),
                message: tr!("dialog.quality_use", tool = found.name, path = found.place.as_str()),
                icon: "alert",
                yes: t("settings.quality_use"),
                danger: true,
                list: Vec::new(),
            };
            let then: OnYes = Box::new(move |_| use_found(&runs, language, kind, &found));
            return Some((question, then));
        }
        let kind =
            [Kind::Formatter, Kind::Checker].into_iter().find(|&k| allow_action(k) == action)?;
        let choice = chosen(project, language, kind);
        let question = Question {
            title: t("dialog.quality_allow_title"),
            message: tr!("dialog.quality_allow", what = runs.shown(&choice)),
            icon: "alert",
            yes: t("settings.quality_allow"),
            danger: true,
            list: Vec::new(),
        };
        let then: OnYes = Box::new(move |_| runs.allow(&choice));
        Some((question, then))
    }
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
