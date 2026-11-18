//! A module's help with the documents it knows (a query console, by the data source it's
//! attached to): completion, the definition and the hover card of what's at the cursor,
//! from data only that module has (the source's schema). Unlike a [`crate::FrameworkSupport`]
//! (per project and framework), what matters here is the document.
//!
//! The app asks the assists for a document's languages ([`DocumentAssist::languages`])
//! before the frameworks and the language. An assist answers synchronously and cheaply,
//! only from what it has already built; what's missing it asks for in the background,
//! wakes the app and bumps its [`DocumentAssist::generation`]: a completion list still open
//! is made again.

use std::fmt::Debug;
use std::path::Path;

use kernel::ProjectId;

use crate::{FrameworkCompletion, HoverInfo, Location};

/// What an assist is asked about: the document, its language at the cursor, its text.
#[derive(Debug, Clone, Copy)]
pub struct AssistContext<'a> {
    /// The open project's identity and root (none: a file opened alone).
    pub project: Option<ProjectId>,
    pub root: Option<&'a Path>,
    /// The document's file (absolute).
    pub path: &'a Path,
    /// Its language at the cursor (`sql`).
    pub language: &'a str,
    /// Its text as in the editor (unsaved changes included).
    pub text: &'a str,
    /// The cursor (bytes).
    pub offset: usize,
    /// The document's version (changes with each edit): what's worked out of the text is
    /// kept under it.
    pub version: u64,
}

/// Where "Go to Definition" leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistTarget {
    /// A place in a file (an alias's, a CTE's, in the same text).
    Location(Location),
    /// A command run (a table's structure or data opened).
    Command(String),
    /// Several places, listed in the order given for the user to choose one (a key defined
    /// in several `.env` files, in the order they're loaded); the list's title says what's
    /// chosen. The first is where a ⌘-click hint points.
    Choose { title: String, places: Vec<AssistPlace> },
    /// A name written as code of another language (a PHP class in a Twig comment's `@var`):
    /// found as that language's engine finds it (the module needs no index).
    Code { language: &'static str, target: crate::Target },
}

/// A place to choose from: what it is (`.env.local`), what it says (`= dev`), where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistPlace {
    pub label: String,
    pub detail: String,
    pub at: Location,
}

impl AssistTarget {
    /// The place it leads to first (a list's first), if it leads to a place.
    pub fn first_place(&self) -> Option<&Location> {
        match self {
            Self::Location(at) => Some(at),
            Self::Choose { places, .. } => places.first().map(|p| &p.at),
            Self::Command(_) | Self::Code { .. } => None,
        }
    }
}

/// A module's help with the documents it knows (see the module's doc). Everything
/// defaults to "nothing to add".
pub trait DocumentAssist: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    /// The languages it's asked for (`["sql"]`).
    fn languages(&self) -> &'static [&'static str];
    /// Whether it's asked at a place of `language`, in a file of `file_language` (what's
    /// embedded at the cursor is whatever the grammar makes of a thing still being typed:
    /// a template's own help is for the whole file).
    fn asked_for(&self, language: &str, file_language: &str) -> bool {
        self.languages().iter().any(|l| *l == language || *l == file_language)
    }
    /// The characters after which it suggests, besides a name's (Twig's `|`).
    fn triggers(&self) -> &'static str {
        ""
    }
    /// Its suggestions: added to the language's or replacing them
    /// ([`FrameworkCompletion::merge`]).
    fn complete(&self, _at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        None
    }
    fn definition(&self, _at: &AssistContext<'_>) -> Option<AssistTarget> {
        None
    }
    fn hover(&self, _at: &AssistContext<'_>) -> Option<HoverInfo> {
        None
    }
    /// The signature of the function called at the cursor (`at.offset`: right after the
    /// callee's name, before its `(`), as the parameter help card shows it
    /// (`substr(text, start, length) → text`).
    fn signature(&self, _at: &AssistContext<'_>) -> Option<String> {
        None
    }
    /// Changes when what it answers from does (a schema read, columns come): a list shown
    /// from it is made again.
    fn generation(&self, _project: Option<ProjectId>) -> u64 {
        0
    }
}
