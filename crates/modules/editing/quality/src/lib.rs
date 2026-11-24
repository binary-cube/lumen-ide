//! Formatting and checking code, as a module: each project's runs (`runner`) follow its
//! saves (what runs on save, as the project chose), the commands run them on demand
//! ("Reformat File", "Check File", "Format Project", "Check Project"); what they found is
//! announced on the project's events (the editor shows the problems), a failure said.
//! "Reformat File" on a language with a formatter of its own (SQL's) and no tool chosen
//! lays the editor's text out with it (the selection, else the file), nothing saved.
//! The tools are the languages' modules' (Code quality context); the choices the project's
//! (`settings`), chosen on the settings' "Code quality" page (`page`).

mod allowed;
mod looked;
mod page;
pub mod runner;
pub mod settings;

use std::path::Path;
use std::sync::Arc;

use code_quality::plan::PlanError;
use code_quality::{Kind, Target};
use command_model::{CommandId, CommandSpec, MenuAt};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::{ProjectId, Subscription};
use workbench_model::{
    ActOutcome, ActionKind, ActionTarget, CommandContribution, CommandHandler, PartAction, PartRef,
    ProjectContext, ProjectService, WorkbenchHost,
};

use runner::ProjectRuns;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("quality"),
    name: "Code quality",
    description: "Formats and checks code with the project's tools, on demand and on save",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "alert", ..extensions::ModuleInfo::BUILT_IN },
};

pub const FORMAT_FILE: CommandId = workbench_model::REFORMAT_FILE;
pub const CHECK_FILE: CommandId = CommandId("quality.check_file");
pub const FORMAT_PROJECT: CommandId = CommandId("quality.format_project");
pub const CHECK_PROJECT: CommandId = CommandId("quality.check_project");

/// The open projects' runs, by their identity.
static PROJECTS: workbench_model::PerProject<ProjectRuns> = workbench_model::PerProject::new();

fn runs_of(id: ProjectId) -> Option<Arc<ProjectRuns>> {
    PROJECTS.get(id)
}

#[derive(Debug, Default)]
pub struct QualityModule;

impl Module for QualityModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&[
            "quality",
            "quality-input",
            "quality-output",
        ]));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(Programs));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(QualityService));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            page::QualitySettings,
        ));
        use workbench_model::menus::sections;
        type Command = (CommandId, &'static str, &'static [&'static str], Run, MenuAt);
        let commands: [Command; 4] = [
            (
                FORMAT_FILE,
                "Reformat File",
                &["CmdOrCtrl+Alt+L"],
                Run(Kind::Formatter, false),
                MenuAt::new(sections::CODE_FORMAT, 10),
            ),
            (
                CHECK_FILE,
                "Check File",
                &[],
                Run(Kind::Checker, false),
                MenuAt::new(sections::CODE_CHECK, 10),
            ),
            (
                FORMAT_PROJECT,
                "Format Project",
                &[],
                Run(Kind::Formatter, true),
                MenuAt::new(sections::CODE_FORMAT, 40),
            ),
            (
                CHECK_PROJECT,
                "Check Project",
                &[],
                Run(Kind::Checker, true),
                MenuAt::new(sections::CODE_CHECK, 20),
            ),
        ];
        for (id, label, shortcuts, run, at) in commands {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: Some(at), shortcuts },
                icon: Some(if run.0 == Kind::Formatter { "braces" } else { "file-check" }),
                // A file's: with a text file open; the project's: with a project.
                handler: workbench_model::Requiring::new(
                    if run.1 {
                        &[command_model::Requires::Project]
                    } else {
                        &[command_model::Requires::Project, command_model::Requires::FileOnDisk]
                    },
                    Arc::new(run),
                ),
            });
        }
        Ok(())
    }
}

/// Follows each project's saves; on the UI thread, announces what the runs found.
#[derive(Debug)]
struct QualityService;

impl ProjectService for QualityService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let runs = Arc::new(ProjectRuns::new(project));
        PROJECTS.insert(project.id, &runs);
        let saved = Arc::clone(&runs);
        let id = project.id;
        // The subscription holds the runs: they live as long as the project, and its entry
        // in the table goes with them.
        let (held, released, closed) = (Arc::clone(&runs), Arc::clone(&runs), runs);
        vec![
            // Only kept here (the saving thread doesn't wait): planned and run on the pool.
            project.events.subscribe(move |e: &document::events::FileSaved| {
                saved.heard_save(&e.path);
            }),
            project.events.subscribe(move |e: &workspace::events::BulkChangeStarted| {
                held.hold(e.id);
            }),
            project.events.subscribe(move |e: &workspace::events::BulkChangeEnded| {
                released.release(e.id);
            }),
            Subscription::on_end(move || {
                closed.close();
                PROJECTS.remove(id);
            }),
        ]
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(runs) = host.project_id().and_then(runs_of) else { return false };
        let done = runs.take_done();
        let any = !done.is_empty();
        for run in done {
            let error = run.outcome.as_ref().err().cloned();
            if let Some(why) = &error {
                host.notify(workbench_model::Notification::error(i18n::tr!(
                    "quality.failed",
                    tool = run.tool.clone(),
                    reason = why.clone()
                )));
            }
            if let (Kind::Checker, Ok(problems)) = (run.kind, run.outcome) {
                host.events().publish(&diagnostics_found(&run.tool, &run.target, problems));
            }
            // Formatted on save, checked after it (as it is now).
            if run.then_check
                && let Err(e) = runs.request(Kind::Checker, run.target, &run.language, false)
            {
                tracing::debug!(error = ?e, "not checked after formatting");
            }
        }
        any
    }
}

/// A checker's problems as the editor shows diagnostics: they replace the target's previous
/// ones, kept apart per tool.
fn diagnostics_found(
    tool: &str,
    target: &Target,
    problems: Vec<code_quality::Problem>,
) -> workbench_model::DiagnosticsFound {
    use workbench_model::{Diagnostic, DiagnosticsScope, Severity};
    let scope = match target {
        Target::File(file) => DiagnosticsScope::File(file.clone()),
        Target::Project => DiagnosticsScope::Project,
    };
    let diagnostics = problems
        .into_iter()
        .map(|p| Diagnostic {
            path: p.path,
            line: p.line,
            column: p.column,
            end: None,
            severity: match p.severity {
                code_quality::Severity::Error => Severity::Error,
                code_quality::Severity::Warning => Severity::Warning,
                code_quality::Severity::Info => Severity::Info,
            },
            message: p.message,
            source: p.tool,
            rule: p.rule,
        })
        .collect();
    workbench_model::DiagnosticsFound { source: tool.to_owned(), scope, diagnostics }
}

/// A command: a `kind` run on the active file, or on the whole project.
#[derive(Debug, Clone, Copy)]
struct Run(Kind, bool);

impl CommandHandler for Run {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id().is_some() && (self.1 || host.active_path().is_some())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Run(kind, whole_project) = *self;
        let Some(runs) = host.project_id().and_then(runs_of) else { return false };
        let Some(file) = host.active_file() else { return false };
        // A locked file isn't formatted (its text, or the file a tool rewrites).
        if kind == Kind::Formatter && !whole_project && host.refuse_if_locked() {
            return false;
        }
        // A file's language with a formatter of its own and no tool chosen: its text laid
        // out in the editor (the selection's, else all of it), nothing saved.
        if kind == Kind::Formatter
            && !whole_project
            && let Some(formatter) = runs.language_of(&file.path).and_then(|l| runs.builtin(&l))
        {
            let scope = match file.selection {
                Some(selection) => code_quality::FormatScope::Range(selection),
                None => code_quality::FormatScope::Whole,
            };
            return workbench_model::format_active(host, formatter, scope, report_format);
        }
        let Ok(relative) = file.path.strip_prefix(runs.root()).map(Path::to_path_buf) else {
            host.notify(workbench_model::Notification::info(i18n::t("quality.outside_project")));
            return false;
        };
        let Some(language) = runs.language_of(&relative) else {
            host.notify(workbench_model::Notification::info(i18n::t("quality.no_language")));
            return false;
        };
        // The tools work on the file on disk: its changes saved first.
        if !host.save_active() {
            return false;
        }
        let target = if whole_project { Target::Project } else { Target::File(relative) };
        match runs.request(kind, target, &language, false) {
            Ok(()) => true,
            Err(why) => {
                let message = match why {
                    PlanError::Nothing => i18n::tr!("quality.nothing", language = language),
                    PlanError::UnknownTool(id) => i18n::tr!("quality.unknown_tool", tool = id),
                    PlanError::NotFound(name) => i18n::tr!("quality.not_found", tool = name),
                    PlanError::NotAllowed(what) => i18n::tr!("quality.not_allowed", what = what),
                };
                host.notify(workbench_model::Notification::warning(message));
                false
            }
        }
    }
}

/// How a built-in formatter's run went, said when there's something to say.
pub fn report_format(host: &mut dyn WorkbenchHost, outcome: workbench_model::FormatOutcome) {
    use workbench_model::{FormatOutcome, Notification};
    let message = match outcome {
        FormatOutcome::Changed { left } | FormatOutcome::Unchanged { left } if left > 0 => {
            i18n::tr!("quality.left_as_written", count = left)
        }
        FormatOutcome::Changed { .. } => return,
        FormatOutcome::Unchanged { .. } => i18n::t("quality.formatted_already"),
        FormatOutcome::Nothing => i18n::t("quality.nothing_to_format"),
        FormatOutcome::Moved => i18n::t("quality.format_moved"),
    };
    host.notify(Notification::info(message));
}

/// The formatters and checkers running: their CPU, memory and disk (with what they start).
#[derive(Debug)]
struct Programs;

impl workbench_model::ResourceReporter for Programs {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        let pids = runner::RUNNING.pids();
        // None running: nothing to show.
        if pids.is_empty() {
            return Vec::new();
        }
        let mut part =
            workbench_model::ResourceUse::new(i18n::t("tasks.quality"), MANIFEST.id.as_str());
        part.threads = vec!["quality", "quality-input", "quality-output"];
        part.group = workbench_model::resources::PROGRAMS;
        part.processes = pids;
        part.detail = i18n::tr!("tasks.programs_detail", count = part.processes.len());
        // Each program can be stopped (asked first: its work is lost).
        part.actions = part
            .processes
            .iter()
            .map(|pid| {
                PartAction::new(STOP_PROGRAM, ActionKind::Stop, ActionTarget::Process(*pid))
                    .confirmed()
            })
            .collect();
        vec![part]
    }

    /// A program stopped by its run (`process::Running::stop`: it and what it started).
    fn act(&self, _part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        match (action, target) {
            (STOP_PROGRAM, ActionTarget::Process(pid)) if runner::RUNNING.stop(pid) => {
                ActOutcome::Done
            }
            _ => ActOutcome::Unsupported,
        }
    }
}

/// The Task Manager's action stopping one of its programs.
const STOP_PROGRAM: &str = "stop-program";

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
