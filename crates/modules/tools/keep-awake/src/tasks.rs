//! Its row in the Task Manager, while it's on: whether the computer is kept awake and for
//! how long, or that it waits for work to run (only while work runs); its thread's CPU.
//! Never an operation: Keep awake is no work of its own (it would keep itself awake).

use std::time::Instant;

use power::Mode;
use workbench_model::{ResourceReporter, ResourceUse};

/// Its module's id (its row).
pub const OWNER: &str = "tools.keep_awake";

#[derive(Debug)]
pub struct Reporter;

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        report_at(Instant::now())
    }
}

/// What it says at `now`.
pub fn report_at(now: Instant) -> Vec<ResourceUse> {
    let config = crate::settings::config();
    if !config.on {
        return Vec::new();
    }
    let mut part = ResourceUse::new(i18n::t("keep_awake.title"), OWNER);
    part.group = workbench_model::resources::MODULES;
    part.threads = vec![crate::watcher::THREAD];
    let when = match config.mode() {
        Mode::WhileWorking => i18n::t("keep_awake.when_working"),
        _ => i18n::t("keep_awake.when_always"),
    };
    part.facts.push((i18n::t("keep_awake.when"), when));
    part.detail = match crate::keeper::keeper().since() {
        Some(since) => {
            let minutes = now.saturating_duration_since(since).as_secs() / 60;
            i18n::tr!("keep_awake.task_held", minutes = minutes)
        }
        None => i18n::t("keep_awake.task_waiting"),
    };
    vec![part]
}

#[cfg(test)]
#[path = "../tests/unit/tasks.rs"]
mod tests;
