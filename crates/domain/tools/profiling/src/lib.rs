//! A profile recording of the app (plan 91), as data: what's kept ([`Recording`], its JSON
//! format), how it's kept small ([`Recording::push`]: a budget of samples, the oldest
//! merged by twos past it), what's let in (names cleaned: [`name`]) and what it says
//! ([`summary`]: the busiest threads and the modules they're for, who asks for frames, what
//! takes the interface's time). Anonymous by construction: only the app's own names and the
//! system's basics (`.claude/rules/privacy.md`). Pure: no disk, no clock, no threads.

pub mod format;
pub mod summary;

pub use format::{
    Busy, Counter, Io, Level, Owners, Recording, Sample, Span, System, ThreadCpu, Trigger, name,
    step,
};
pub use summary::{Finding, Severity, Summary, summary};
