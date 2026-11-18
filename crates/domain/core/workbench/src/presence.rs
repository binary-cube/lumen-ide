//! Who is there: whether some module answers a neutral contract (a request, a command, a
//! view's extension), without knowing which; views ask here, having no host. What depends on an answer is shown only
//! while there is one, so a module taken out leaves no button that does nothing. Told once by
//! the app after the modules start; until then (and in a module's own tests) everything counts
//! as there.

use std::collections::HashSet;
use std::sync::{PoisonError, RwLock};

use command_model::CommandId;
use kernel::Request;

use crate::AnswerBook;

type HasView = Box<dyn Fn(&str) -> bool + Send + Sync>;

struct Present {
    commands: HashSet<&'static str>,
    answers: std::sync::Arc<AnswerBook>,
    has_view: HasView,
}

static PRESENT: RwLock<Option<Present>> = RwLock::new(None);

/// What the started modules answer (the app, once they started).
pub fn set(
    commands: impl IntoIterator<Item = CommandId>,
    answers: std::sync::Arc<AnswerBook>,
    has_view: impl Fn(&str) -> bool + Send + Sync + 'static,
) {
    *PRESENT.write().unwrap_or_else(PoisonError::into_inner) = Some(Present {
        commands: commands.into_iter().map(|c| c.0).collect(),
        answers,
        has_view: Box::new(has_view),
    });
}

fn asked(question: impl FnOnce(&Present) -> bool) -> bool {
    PRESENT.read().unwrap_or_else(PoisonError::into_inner).as_ref().is_none_or(question)
}

/// Someone runs command `id`.
pub fn command(id: CommandId) -> bool {
    asked(|p| p.commands.contains(id.0))
}

/// Someone answers requests of type `R`.
pub fn request<R: Request>() -> bool {
    asked(|p| p.answers.has::<R>())
}

/// Someone opens views with `extension`.
pub fn view(extension: &str) -> bool {
    asked(|p| (p.has_view)(extension))
}

type Opener = Box<dyn Fn(&str) -> Option<&'static str> + Send + Sync>;

static OPENER: RwLock<Option<Opener>> = RwLock::new(None);

/// Who opens a file by its extension (the app, once the modules started): the id of the
/// viewer chosen for it (`None`: none, the text editor).
pub fn set_opener(opener: impl Fn(&str) -> Option<&'static str> + Send + Sync + 'static) {
    *OPENER.write().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(opener));
}

/// The viewer (its provider's id) a file with `extension` opens in, as the app chooses it
/// among the modules' (a hidden module's last): a module offering "Open in …" beside the
/// default hides it while it *is* the default. `None`: none, or not told yet.
pub fn opener(extension: &str) -> Option<&'static str> {
    OPENER.read().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(|f| f(extension))
}

/// Someone shows comparisons.
pub fn compare() -> bool {
    view(diff::COMPARE_VIEW)
}

#[cfg(test)]
#[path = "../tests/unit/presence.rs"]
mod tests;
