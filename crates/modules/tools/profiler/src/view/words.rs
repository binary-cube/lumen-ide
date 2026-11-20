//! What a recording says, in words: each finding as a sentence in the user's language, the
//! names of the causes, and the whole summary as text (what "Copy Summary" puts on the
//! clipboard: Markdown, to paste into a bug report).

use i18n::{t, tr};
use profiling::{Finding, Recording, Severity, Summary};

/// A cause's name (`poll`, `deadline`…) as the user reads it.
pub fn cause(id: &str) -> String {
    let key = format!("profiler.cause_{id}");
    let found = t(&key);
    if found == key { id.to_owned() } else { found }
}

/// Whose a thread is, said after its name (`main · app`).
fn whose(owner: Option<&String>) -> String {
    owner.map_or_else(String::new, |o| format!(" ({o})"))
}

/// A finding as a sentence.
pub fn finding(f: &Finding) -> String {
    match f {
        Finding::BusyWhileIdle { cpu, inputs } => tr!(
            "profiler.find_idle",
            cpu = format!("{cpu:.0}%").as_str(),
            inputs = format!("{inputs:.1}").as_str()
        ),
        Finding::BusyThread { thread, owner, cpu } => tr!(
            "profiler.find_thread",
            thread = format!("{thread}{}", whose(owner.as_ref())).as_str(),
            cpu = format!("{cpu:.0}%").as_str()
        ),
        Finding::FrameAsker { owner, cause: why, rate } => tr!(
            "profiler.find_frames",
            owner = owner.as_str(),
            rate = format!("{rate:.0}").as_str(),
            cause = cause(why).as_str()
        ),
        Finding::SlowPart { owner, what, share, longest_ms } => tr!(
            "profiler.find_slow",
            owner = owner.as_str(),
            what = what.as_str(),
            share = format!("{share:.0}%").as_str(),
            longest = format!("{longest_ms:.0} ms").as_str()
        ),
        Finding::BusyIo { owner, network, disk } => tr!(
            "profiler.find_io",
            owner = owner.as_str(),
            network = format!("{}/s", i18n::bytes(*network as u64)).as_str(),
            disk = format!("{}/s", i18n::bytes(*disk as u64)).as_str()
        ),
        Finding::LongFrame { ms } => {
            tr!("profiler.find_long_frame", ms = format!("{ms:.0} ms").as_str())
        }
    }
}

/// What to do about a finding: where to look (the module, the part, by the names in our
/// code) and what usually fixes it.
pub fn advice(f: &Finding) -> String {
    match f {
        Finding::BusyWhileIdle { .. } => t("profiler.advice_idle"),
        Finding::BusyThread { thread, owner, .. } => tr!(
            "profiler.advice_thread",
            thread = thread.as_str(),
            owner = owner.as_deref().unwrap_or("app")
        ),
        Finding::FrameAsker { owner, cause: why, .. } => {
            let key = match why.as_str() {
                "poll" => "profiler.advice_poll",
                "animation" => "profiler.advice_animation",
                "deadline" => "profiler.advice_deadline",
                "wake" => "profiler.advice_wake",
                _ => "profiler.advice_frames",
            };
            tr!(key, owner = owner.as_str())
        }
        Finding::SlowPart { owner, what, .. } => {
            tr!("profiler.advice_slow", owner = owner.as_str(), what = what.as_str())
        }
        Finding::LongFrame { .. } => t("profiler.advice_long_frame"),
        Finding::BusyIo { owner, .. } => tr!("profiler.advice_io", owner = owner.as_str()),
    }
}

/// The finding's level, for its icon and color.
pub fn level(f: &Finding) -> ui_components::severity::Level {
    match f.severity() {
        Severity::High => ui_components::severity::Level::Critical,
        Severity::Medium => ui_components::severity::Level::Warning,
    }
}

/// `total` in its unit (`count`, `bytes`, `us`) as read.
pub fn amount(total: u64, unit: &str) -> String {
    match unit {
        "bytes" => i18n::bytes(total),
        "us" => format!("{:.1} ms", total as f64 / 1000.0),
        _ => total.to_string(),
    }
}

/// `per_second` in its unit, a second.
pub fn rate(per_second: f64, unit: &str) -> String {
    match unit {
        "bytes" => format!("{:.2} MB/s", per_second / (1024.0 * 1024.0)),
        "us" => format!("{:.1} ms/s", per_second / 1000.0),
        _ => format!("{per_second:.1} / s"),
    }
}

/// A length as `1:05` (minutes and seconds) or `42 s`.
pub fn length(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s >= 60 { format!("{}:{:02}", s / 60, s % 60) } else { format!("{s} s") }
}

/// The recording's line about where it ran: the system, its session, the app's version.
pub fn ran_on(recording: &Recording) -> String {
    let s = &recording.system;
    let mut parts = vec![format!("{} {}", s.os, s.os_version).trim().to_owned()];
    if !s.session.is_empty() && s.session != s.os {
        parts.push(s.session.clone());
    }
    parts.push(s.arch.clone());
    if s.cores > 0 {
        parts.push(tr!("profiler.cores", cores = s.cores.to_string().as_str()));
    }
    if !s.app_version.is_empty() {
        parts.push(format!("{} {}", s.app_version, s.build).trim().to_owned());
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

/// The whole summary as Markdown (a bug report's).
pub fn as_text(recording: &Recording, s: &Summary) -> String {
    let mut out = format!(
        "## {}\n\n{} · {}\n\n",
        t("profiler.view_title"),
        recording.started,
        ran_on(recording)
    );
    out.push_str(&format!(
        "- {}: {:.1}%\n- {}: {:.1}\n- {}: {:.1} ms\n- {}: {}\n",
        t("profiler.cpu"),
        s.cpu,
        t("profiler.fps"),
        s.fps,
        t("profiler.frame_longest"),
        s.frame_longest_ms,
        t("profiler.memory_peak"),
        i18n::bytes(s.memory_peak)
    ));
    if !s.findings.is_empty() {
        out.push_str(&format!("\n### {}\n\n", t("profiler.findings")));
        for f in &s.findings {
            out.push_str(&format!("- {}\n  → {}\n", finding(f), advice(f)));
        }
    }
    out.push_str(&format!("\n### {}\n\n", t("profiler.modules")));
    for (module, cpu) in s.modules.iter().take(10) {
        out.push_str(&format!("- {module}: {cpu:.1}%\n"));
    }
    out.push_str(&format!("\n### {}\n\n", t("profiler.triggers")));
    for (owner, why, rate) in s.triggers.iter().take(10) {
        out.push_str(&format!("- {owner} · {}: {rate:.1}/s\n", cause(why)));
    }
    out.push_str(&format!("\n### {}\n\n", t("profiler.busy")));
    for (owner, what, share, calls, longest) in s.busy.iter().take(10) {
        out.push_str(&format!("- {owner} · {what}: {share:.1}%, {calls}×, ≤{longest:.1} ms\n"));
    }
    out
}
