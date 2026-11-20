//! A project's runs: planned from its choices (the Code quality context), started in the
//! background through the shared process runner, never two on the same target at once
//! (one asked for meanwhile runs after, once), their outcome gathered and announced on the
//! UI thread (`take_done`, from the module's `poll`): the problems found, the error said.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use code_quality::plan::{Lookup, PlanError};
use code_quality::{
    Invocation, Kind, Problem, Program, QualityTool, Runs, Start, Target, ToolOutput, plan,
};
use settings_model::SettingsService;

/// The formatters and checkers running now, every project's (the Task Manager measures
/// them).
pub(crate) static RUNNING: std::sync::LazyLock<Arc<process::Running>> =
    std::sync::LazyLock::new(Arc::default);

/// A run's longest time (a whole project's check can take a while).
const TIMEOUT: Duration = Duration::from_secs(180);

/// Runs at once per project (each one a program: a checker can take a core or more); the
/// others wait their turn.
const AT_ONCE: usize = 2;

/// The saves heard, waiting to be planned (off the thread that saved: finding a program
/// reads `PATH`); held while a batch of changes runs, taken as one when it ends.
#[derive(Debug, Default)]
struct Saves {
    paths: Vec<PathBuf>,
    /// The batches of changes running (their identities).
    held: Vec<u128>,
    /// A job is on its way to plan them.
    scheduled: bool,
}

/// A run that ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub kind: Kind,
    pub target: Target,
    pub tool: String,
    pub language: String,
    /// The problems (a checker's), or why it failed.
    pub outcome: Result<Vec<Problem>, String>,
    /// A formatter run on save with checking on save too: the file is checked after it.
    pub then_check: bool,
}

/// A run planned and waiting for its turn.
#[derive(Debug, Clone)]
struct Planned {
    invocation: Invocation,
    tool: String,
    language: String,
    then_check: bool,
}

/// The runs of one project.
pub struct ProjectRuns {
    root: PathBuf,
    settings: Option<Arc<SettingsService>>,
    language_of: workbench_model::LanguageOf,
    tools: Vec<Arc<dyn QualityTool>>,
    /// The formatters the languages' modules built in.
    formatters: Vec<Arc<dyn code_quality::TextFormatter>>,
    /// What the user allowed to run (the settings may have come with the project).
    allowed: crate::allowed::Allowed,
    wake: Arc<dyn Fn() + Send + Sync>,
    runs: Mutex<Runs>,
    waiting: Mutex<HashMap<(Target, Kind), Planned>>,
    done: Mutex<Vec<Done>>,
    saves: Mutex<Saves>,
    /// Where the planning and the runs happen (its threads end when idle).
    pool: kernel::work::Pool,
    /// Cancelled when the project closes: what hasn't started doesn't.
    closed: kernel::work::Cancel,
    /// The Node a project's Node script runs on: the system's, else the app's own.
    node: code_quality::runtime::NodeFinder,
}

impl std::fmt::Debug for ProjectRuns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectRuns").field("root", &self.root).finish_non_exhaustive()
    }
}

impl ProjectRuns {
    pub fn new(project: &workbench_model::ProjectContext<'_>) -> Self {
        Self {
            root: project.root.to_path_buf(),
            settings: project.settings.clone(),
            language_of: Arc::clone(&project.language_of),
            tools: project.quality_tools.clone(),
            formatters: project.text_formatters.clone(),
            allowed: crate::allowed::Allowed::new(Some(project.storage)),
            wake: Arc::clone(&project.wake),
            runs: Mutex::default(),
            waiting: Mutex::default(),
            done: Mutex::default(),
            saves: Mutex::default(),
            pool: kernel::work::Pool::new("quality", AT_ONCE, kernel::work::IDLE),
            closed: kernel::work::Cancel::new(),
            node: Arc::new(|| process::which("node").or_else(code_quality::runtime::node)),
        }
    }

    /// Its Node found by `node` instead (the tests': a stand-in, never the machine's).
    #[cfg(all(test, unix))]
    pub(crate) fn use_node(&mut self, node: code_quality::runtime::NodeFinder) {
        self.node = node;
    }

    /// The project closed: what's waiting won't start.
    pub fn close(&self) {
        self.closed.cancel();
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project's settings (where its choices are kept).
    pub fn settings(&self) -> Option<Arc<SettingsService>> {
        self.settings.clone()
    }

    /// The language of a file of the project (absolute or relative).
    pub fn language_of(&self, path: &Path) -> Option<String> {
        (self.language_of)(&self.root.join(path))
    }

    /// The project's choice for `language`.
    pub fn choice(&self, language: &str) -> code_quality::LanguageChoice {
        self.settings
            .as_deref()
            .map(|settings| crate::settings::choice(settings, language))
            .unwrap_or_default()
    }

    /// A `kind` run of the project's choice for `language` on `target`: started now, after
    /// the one running on it, or not at all (said why: nothing chosen, not found).
    pub fn request(
        self: &Arc<Self>,
        kind: Kind,
        target: Target,
        language: &str,
        then_check: bool,
    ) -> Result<(), PlanError> {
        let choice = self.choice(language);
        let chosen = match kind {
            Kind::Formatter => &choice.formatter,
            Kind::Checker => &choice.checker,
        };
        let invocation = self.plan(chosen, kind, &target)?;
        let tool = match (&invocation.program, chosen) {
            (Program::Shell(line), _) => line.clone(),
            (_, code_quality::Choice::Tool(id)) => self
                .tools
                .iter()
                .find(|t| t.id() == id && t.kind() == kind)
                .map_or_else(|| id.clone(), |t| t.name().to_owned()),
            (Program::Exec(path), _) => path.display().to_string(),
        };
        let planned = Planned { invocation, tool, language: language.to_owned(), then_check };
        let key = (planned.invocation.target.clone(), kind);
        let start = self.runs.lock().map_or(Start::Queued, |mut r| r.ask(&key.0, kind));
        match start {
            Start::Now => self.spawn(kind, planned),
            Start::Queued => {
                if let Ok(mut waiting) = self.waiting.lock() {
                    waiting.insert(key, planned);
                }
            }
        }
        Ok(())
    }

    /// The run of `chosen` on `target`, its tool found in the project or in `PATH`.
    fn plan(
        &self,
        chosen: &code_quality::Choice,
        kind: Kind,
        target: &Target,
    ) -> Result<Invocation, PlanError> {
        if !self.allowed.allows(chosen) {
            return Err(PlanError::NotAllowed(self.shown(chosen)));
        }
        let exists = |p: &Path| p.is_file();
        let find = |name: &str| process::which(name);
        // A project's Node script: on the system's Node, else the app's own.
        let node = || (self.node)();
        let lookup = Lookup { exists: &exists, find: &find, node: &node };
        plan(chosen, kind, &self.tools, target, &self.root, &lookup)
    }

    /// What a `kind` run on a file of `language` would start (the settings show it), the
    /// file written `{file}`.
    pub fn preview(&self, kind: Kind, language: &str) -> Result<Invocation, PlanError> {
        let choice = self.choice(language);
        let chosen = match kind {
            Kind::Formatter => &choice.formatter,
            Kind::Checker => &choice.checker,
        };
        self.plan(chosen, kind, &Target::File(PathBuf::from("{file}")))
    }

    /// What `choice` runs, as shown: a tool's name, a binary's path, a command line.
    pub(crate) fn shown(&self, choice: &code_quality::Choice) -> String {
        use code_quality::Choice;
        match choice {
            Choice::None => String::new(),
            Choice::Tool(id) => self
                .tools
                .iter()
                .find(|t| t.id() == id)
                .map_or_else(|| id.clone(), |t| t.name().to_owned()),
            Choice::Binary { path, .. } => path.clone(),
            Choice::Command { line, .. } => line.clone(),
        }
    }

    /// Whether `choice` may run (the user allowed it).
    pub(crate) fn allows(&self, choice: &code_quality::Choice) -> bool {
        self.allowed.allows(choice)
    }

    /// Allows `choice` to run from now on (the user chose it, or allowed it).
    pub(crate) fn allow(&self, choice: &code_quality::Choice) {
        self.allowed.allow(choice);
    }

    /// The tools the languages' modules contributed.
    pub fn tools(&self) -> &[Arc<dyn QualityTool>] {
        &self.tools
    }

    /// The formatter built in for `language`, used when the project chose none for it.
    pub fn builtin(&self, language: &str) -> Option<Arc<dyn code_quality::TextFormatter>> {
        if self.choice(language).formatter != code_quality::Choice::None {
            return None;
        }
        self.formatters.iter().find(|f| f.languages().contains(&language)).cloned()
    }

    /// Whether these are the runs of the project whose settings are `settings`.
    pub fn uses(&self, settings: &SettingsService) -> bool {
        self.settings.as_deref().is_some_and(|s| std::ptr::eq(s, settings))
    }

    /// Starts a run on its thread; when it ends, those waiting for it start.
    fn spawn(self: &Arc<Self>, kind: Kind, planned: Planned) {
        let runs = Arc::clone(self);
        self.pool.spawn(kernel::work::Priority::Interactive, Some(&self.closed), move || {
            let outcome = runs.run(kind, &planned.invocation);
            let target = planned.invocation.target.clone();
            if let Ok(mut done) = runs.done.lock() {
                done.push(Done {
                    kind,
                    target: target.clone(),
                    tool: planned.tool.clone(),
                    language: planned.language.clone(),
                    outcome,
                    then_check: planned.then_check,
                });
            }
            (runs.wake)();
            let next = runs.runs.lock().map(|mut r| r.finished(&target, kind)).unwrap_or_default();
            for target in next {
                let waiting = runs.waiting.lock().ok().and_then(|mut w| w.remove(&(target, kind)));
                if let Some(planned) = waiting {
                    runs.spawn(kind, planned);
                }
            }
        });
    }

    /// Runs it: a formatter's error, or a checker's problems (paths relative to the project).
    fn run(&self, kind: Kind, invocation: &Invocation) -> Result<Vec<Problem>, String> {
        let spec = match &invocation.program {
            Program::Exec(path) => process::Run::new(path, &invocation.args),
            Program::Shell(line) => match &invocation.target {
                // `{file}`: given in a variable, never written into the line.
                Target::File(file) => process::Run::shell(line)
                    .env(code_quality::plan::FILE_VARIABLE, file.as_os_str().to_owned()),
                Target::Project => process::Run::shell(line),
            },
        };
        // The file's text on its stdin (a formatter prints it formatted).
        let given = match (&invocation.target, invocation.stdin) {
            (Target::File(file), true) => Some(
                std::fs::read_to_string(self.root.join(file))
                    .map_err(|e| format!("{}: {e}", file.display()))?,
            ),
            _ => None,
        };
        let spec = match &given {
            Some(text) => spec.stdin(text.as_bytes()),
            None => spec,
        };
        let spec =
            spec.dir(&self.root).timeout(TIMEOUT).listed_in(Arc::clone(&RUNNING)).owner("quality");
        let finished = process::run(&spec).map_err(|e| e.to_string())?;
        let output = ToolOutput {
            code: finished.code,
            stdout: finished.stdout.clone(),
            stderr: finished.stderr.clone(),
        };
        match kind {
            Kind::Formatter if finished.success() => {
                // Formatted on stdout: written back when it differs (and isn't empty: a
                // tool that printed nothing changed nothing; nor cut at the output's limit).
                if let (Some(before), Target::File(file)) = (&given, &invocation.target)
                    && !finished.stdout.is_empty()
                    && finished.stdout != *before
                {
                    if finished.stdout.len() >= process::MAX_OUTPUT {
                        return Err(format!("{}: too big to format", file.display()));
                    }
                    files::write_atomic(&self.root.join(file), finished.stdout.as_bytes())
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                }
                Ok(Vec::new())
            }
            Kind::Formatter => Err(finished.summary()),
            Kind::Checker => {
                let reader = invocation
                    .reads_as
                    .as_deref()
                    .and_then(|id| self.tools.iter().find(|t| t.id() == id && t.kind() == kind));
                let problems = match reader {
                    Some(tool) => tool.problems(&output),
                    None => code_quality::read_lines(&output, "script"),
                };
                // Nothing read and a failed exit: it didn't check (a config error…).
                if problems.is_empty() && !finished.success() && !finished.summary().is_empty() {
                    return Err(finished.summary());
                }
                Ok(problems.into_iter().map(|p| self.relative(p)).collect())
            }
        }
    }

    /// A problem's path relative to the project (tools often print it absolute).
    fn relative(&self, mut problem: Problem) -> Problem {
        let canonical = self.root.canonicalize().ok();
        let roots = std::iter::once(self.root.as_path()).chain(canonical.as_deref());
        for root in roots {
            if let Ok(rest) = problem.path.strip_prefix(root) {
                problem.path = rest.to_path_buf();
                break;
            }
        }
        problem
    }

    /// The runs ended since the last call.
    pub fn take_done(&self) -> Vec<Done> {
        self.done.lock().map(|mut d| std::mem::take(&mut *d)).unwrap_or_default()
    }

    /// A file of the project was saved (heard on the thread that saved): kept, planned on
    /// the pool — at once, or when the batch of changes running ends (each file once).
    pub fn heard_save(self: &Arc<Self>, path: &Path) {
        let Ok(mut saves) = self.saves.lock() else { return };
        if !saves.paths.iter().any(|p| p == path) {
            saves.paths.push(path.to_path_buf());
        }
        self.schedule(&mut saves);
    }

    /// A batch of changes started: the saves wait for its end.
    pub fn hold(&self, id: u128) {
        if let Ok(mut saves) = self.saves.lock() {
            saves.held.push(id);
        }
    }

    /// A batch ended: the saves heard meanwhile are planned.
    pub fn release(self: &Arc<Self>, id: u128) {
        let Ok(mut saves) = self.saves.lock() else { return };
        saves.held.retain(|h| *h != id);
        self.schedule(&mut saves);
    }

    fn schedule(self: &Arc<Self>, saves: &mut Saves) {
        if saves.scheduled || !saves.held.is_empty() || saves.paths.is_empty() {
            return;
        }
        saves.scheduled = true;
        let runs = Arc::clone(self);
        self.pool.spawn(kernel::work::Priority::Interactive, Some(&self.closed), move || {
            let paths = runs
                .saves
                .lock()
                .map(|mut s| {
                    s.scheduled = false;
                    std::mem::take(&mut s.paths)
                })
                .unwrap_or_default();
            for path in paths {
                runs.saved(&path);
            }
        });
    }

    /// A file of the project was saved: what runs on save for its language.
    pub fn saved(self: &Arc<Self>, path: &Path) {
        let Ok(relative) = path.strip_prefix(&self.root) else { return };
        let Some(language) = self.language_of(relative) else { return };
        let choice = self.choice(&language);
        let target = Target::File(relative.to_path_buf());
        if choice.format_on_save && !choice.formatter.is_none() {
            let then_check = choice.check_on_save && !choice.checker.is_none();
            if let Err(e) = self.request(Kind::Formatter, target, &language, then_check) {
                tracing::debug!(error = ?e, "not formatted on save");
            }
        } else if choice.check_on_save
            && !choice.checker.is_none()
            && let Err(e) = self.request(Kind::Checker, target, &language, false)
        {
            tracing::debug!(error = ?e, "not checked on save");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/runner.rs"]
mod tests;
