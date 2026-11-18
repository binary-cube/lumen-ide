//! The refactoring context's domain events.

use std::path::PathBuf;

use kernel::DomainEvent;

/// A refactoring was applied (or undone): the files whose text changed (under their names
/// now), those of them changed only in their open documents (not on disk until saved), and
/// the entries renamed / moved (`from` → `to`; a folder's files one by one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefactoringApplied {
    pub changed: Vec<PathBuf>,
    pub unsaved: Vec<PathBuf>,
    pub renamed: Vec<(PathBuf, PathBuf)>,
}

impl DomainEvent for RefactoringApplied {}
