//! A run, planned: which program (found in the project, in `PATH`, given, or the shell for a
//! command line), its arguments, its folder, how its output reads.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{Choice, Kind, QualityTool};

/// What a run is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    /// One file, relative to the project's root.
    File(PathBuf),
    /// The whole project.
    Project,
}

/// What's started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// A program, with the invocation's arguments.
    Exec(PathBuf),
    /// A command line for the system's shell (the arguments are in it).
    Shell(String),
}

/// A run, ready to start (in the project's folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: Program,
    pub args: Vec<String>,
    /// What it runs on (a command line without `{file}` runs on the whole project).
    pub target: Target,
    /// The tool whose output format to read it with (`None`: lines).
    pub reads_as: Option<String>,
    /// The file's text is given on its stdin (a formatter prints it formatted).
    pub stdin: bool,
}

/// Why nothing can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// None chosen.
    Nothing,
    /// The chosen tool isn't known (its module is off) — its id.
    UnknownTool(String),
    /// The tool wasn't found in the project nor in `PATH` — its name.
    NotFound(String),
    /// The project's settings choose it, but the user hasn't allowed it yet (the settings
    /// may have come with the project) — what it would run.
    NotAllowed(String),
}

/// The run of `choice` (a `kind` tool) on `target` in the project at `root`; tools are
/// found in it or in `PATH` (`lookup`).
pub fn plan(
    choice: &Choice,
    kind: Kind,
    tools: &[Arc<dyn QualityTool>],
    target: &Target,
    root: &Path,
    lookup: &Lookup<'_>,
) -> Result<Invocation, PlanError> {
    let tool = |id: &str| tools.iter().find(|t| t.id() == id && t.kind() == kind);
    match choice {
        Choice::None => Err(PlanError::Nothing),
        Choice::Tool(id) => {
            let tool = tool(id).ok_or_else(|| PlanError::UnknownTool(id.clone()))?;
            let stdin = tool.stdin() && matches!(target, Target::File(_));
            // The project's Node script, run by a Node (the system's, else the app's).
            let script = tool
                .scripts()
                .iter()
                .map(|s| root.join(s))
                .find(|s| (lookup.exists)(s))
                .and_then(|script| Some(((lookup.node)()?, script)));
            if let Some((node, script)) = script {
                let mut args = vec![script.to_string_lossy().into_owned()];
                args.extend(tool.args(target, root));
                return Ok(Invocation {
                    program: Program::Exec(node),
                    args,
                    target: target.clone(),
                    reads_as: Some(id.clone()),
                    stdin,
                });
            }
            let program = locate(tool.candidates(), root, lookup)
                .ok_or_else(|| PlanError::NotFound(tool.name().to_owned()))?;
            Ok(Invocation {
                program: Program::Exec(program),
                args: tool.args(target, root),
                target: target.clone(),
                reads_as: Some(id.clone()),
                stdin,
            })
        }
        Choice::Binary { path, reads_as } => {
            let given = Path::new(path);
            let program = if given.is_absolute() { given.to_path_buf() } else { root.join(given) };
            // Its arguments are the format's tool's, else just the file.
            let args = match (reads_as.as_deref().and_then(tool), target) {
                (Some(tool), _) => tool.args(target, root),
                (None, Target::File(file)) => vec![file.to_string_lossy().into_owned()],
                (None, Target::Project) => Vec::new(),
            };
            Ok(Invocation {
                program: Program::Exec(program),
                args,
                target: target.clone(),
                reads_as: reads_as.clone(),
                stdin: false,
            })
        }
        Choice::Command { line, reads_as } => {
            let (line, target) = match target {
                // The file through a variable: the shell never reads its name as code.
                Target::File(_) if line.contains("{file}") => {
                    (line.replace("{file}", FILE_IN_SHELL), target.clone())
                }
                // Without `{file}`: it runs on the whole project.
                _ => (line.clone(), Target::Project),
            };
            Ok(Invocation {
                program: Program::Shell(line),
                args: Vec::new(),
                target,
                reads_as: reads_as.clone(),
                stdin: false,
            })
        }
    }
}

/// How tools are found: `exists` (a path in the project), `find` (a name in `PATH`),
/// `node` (the Node a tool's script runs on: the system's, else the app's).
pub struct Lookup<'a> {
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub find: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub node: &'a dyn Fn() -> Option<PathBuf>,
}

impl std::fmt::Debug for Lookup<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lookup").finish_non_exhaustive()
    }
}

/// The first candidate found: a path (with a `/`) in the project, else a name in `PATH`.
fn locate(candidates: &[&str], root: &Path, lookup: &Lookup<'_>) -> Option<PathBuf> {
    candidates.iter().find_map(|candidate| {
        if candidate.contains('/') {
            let path = root.join(candidate);
            (lookup.exists)(&path).then_some(path)
        } else {
            (lookup.find)(candidate)
        }
    })
}

/// The environment variable holding the file a command line runs on (its `{file}`).
pub const FILE_VARIABLE: &str = "EDITOR_FILE";

/// `{file}` as the shell reads it: the variable, quoted (one word, its value never parsed).
const FILE_IN_SHELL: &str = if cfg!(windows) { "\"%EDITOR_FILE%\"" } else { "\"$EDITOR_FILE\"" };

#[cfg(test)]
#[path = "../tests/unit/plan.rs"]
mod tests;
