//! What a project uses for a language: its formatter and its checker (each none, a tool its
//! language's module knows, a binary, or a command / script of the project), and whether
//! they run when a file is saved.

/// One choice (a formatter's or a checker's).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Choice {
    #[default]
    None,
    /// A tool a module knows, by its id, found in the project or in `PATH`.
    Tool(String),
    /// A program at this path (relative to the project, or absolute); its output read as
    /// tool `reads_as`'s (its id), else as lines `file:line:column: message`.
    Binary { path: String, reads_as: Option<String> },
    /// A command line run in the project's folder by the system's shell
    /// (`./scripts/linux/shell.sh composer cs:fix`): with `{file}`, for the file; without,
    /// for the whole project. Its output read as `reads_as`'s, else as lines.
    Command { line: String, reads_as: Option<String> },
}

impl Choice {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// The tool whose output format it reads (`Tool`'s own, or the one named).
    pub fn reads_as(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Tool(id) => Some(id),
            Self::Binary { reads_as, .. } | Self::Command { reads_as, .. } => reads_as.as_deref(),
        }
    }
}

/// A language's formatter and checker in a project, and what runs on save.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageChoice {
    pub formatter: Choice,
    pub checker: Choice,
    pub format_on_save: bool,
    pub check_on_save: bool,
}
