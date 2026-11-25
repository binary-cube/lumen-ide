//! Whether CEF's subprocesses run in Chromium's sandbox:
//! - macOS: always (each helper enters it, `helper.rs`);
//! - Linux: where the system lets a user make namespaces, Chromium's own way without a
//!   setuid helper (which a downloaded engine can't install). Some systems turn that off
//!   (a kernel switch, a zero limit, Ubuntu's AppArmor restriction): there, none;
//! - Windows: none yet (it needs CEF's bootstrap program, plan 99).
//!
//! `EDITOR_WEB_NO_SANDBOX=1` turns it off anyway (a system where the check is wrong).

/// The variable that turns the sandbox off.
const OFF: &str = "EDITOR_WEB_NO_SANDBOX";

/// Whether the subprocesses are sandboxed here.
pub fn wanted() -> bool {
    if std::env::var_os(OFF).is_some_and(|v| v == "1") {
        return false;
    }
    if cfg!(target_os = "macos") {
        return true;
    }
    if cfg!(target_os = "linux") {
        let read = |path: &str| std::fs::read_to_string(path).ok();
        return namespaces_allowed(read, |path| std::path::Path::new(path).exists());
    }
    false
}

/// Whether a user may make the namespaces Chromium's sandbox needs, from the system's
/// switches (`read`: a file's text, `None` where it isn't; `exists`: whether a path is there
/// at all — a namespace's own entry is read by nobody).
pub fn namespaces_allowed(
    read: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> bool {
    let value = |path: &str| read(path).map(|text| text.trim().to_owned());
    // Debian's older kernels: a switch of their own.
    if value("/proc/sys/kernel/unprivileged_userns_clone").as_deref() == Some("0") {
        return false;
    }
    // A limit of none.
    let limit = value("/proc/sys/user/max_user_namespaces");
    if limit.as_deref() == Some("0") {
        return false;
    }
    // Ubuntu (23.10 and later): made, but without the rights the sandbox needs, unless a
    // profile of the system's allows the program.
    if value("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").as_deref() == Some("1") {
        return false;
    }
    // No namespaces at all.
    limit.is_some() || exists("/proc/self/ns/user")
}

#[cfg(test)]
#[path = "../tests/unit/sandbox.rs"]
mod tests;
