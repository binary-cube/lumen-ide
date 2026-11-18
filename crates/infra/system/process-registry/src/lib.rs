//! The programs the app started, known after a crash (plan 83).
//!
//! While the app runs, [`Registry`] notes in `<data>/processes/<instance>.toml` each program
//! it started that lives long (told by `process::launched`): its pid and start time, its
//! kind, its name (never its arguments) and its project. At a normal exit the file goes.
//!
//! At the next start, [`survey`] reads the files of instances no longer running: what they
//! started and still runs (the same start time: a pid used again is another program) is
//! either a helper (a server, git…: stopped, with what it started) or the user's work (a
//! terminal, an agent, a task: found, with its CPU now, for the app to ask about — kept in
//! this instance's file until answered, so a second crash asks again).

pub mod record;
pub mod registry;
pub mod row;
pub mod stop;
pub mod survey;

pub use record::{InstanceFile, Record};
pub use registry::Registry;
pub use row::{Row, row};
pub use survey::{Found, Survey, survey};

/// The registry's folder in the app's data folder `data`.
pub fn dir(data: &std::path::Path) -> std::path::PathBuf {
    data.join("processes")
}
