//! What a server asks to change (a fix, a formatting, its own `applyEdit`) as a refactoring's
//! plan: applied by the workbench like any refactoring — open files edited (unsaved, one
//! undo step each), closed ones written, files renamed — and undone at once.

use code_intelligence::servers::WorkspaceChange;
use refactor::Plan;

/// `change` as a plan, every place chosen (the domain's conversion, shared with the app's
/// context actions).
pub fn plan(change: &WorkspaceChange) -> Plan {
    refactor::plan_of(change)
}

#[cfg(test)]
#[path = "../tests/unit/edits.rs"]
mod tests;
