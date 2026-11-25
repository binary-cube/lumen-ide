use super::*;

/// The system's switches as files.
fn system<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |path| files.iter().find(|(p, _)| *p == path).map(|(_, text)| format!("{text}\n"))
}

/// Whether the namespaces' own entry is there (never read: the system refuses to).
fn entry(there: bool) -> impl Fn(&str) -> bool {
    move |path| there && path == "/proc/self/ns/user"
}

/// Namespaces a user may make: sandboxed; any switch turning them off: not.
#[test]
fn the_sandbox_follows_the_systems_switches() {
    let open = [("/proc/sys/user/max_user_namespaces", "63000")];
    assert!(namespaces_allowed(system(&open), entry(false)));
    let debian = [
        ("/proc/sys/user/max_user_namespaces", "63000"),
        ("/proc/sys/kernel/unprivileged_userns_clone", "0"),
    ];
    assert!(!namespaces_allowed(system(&debian), entry(false)));
    let ubuntu = [
        ("/proc/sys/user/max_user_namespaces", "63000"),
        ("/proc/sys/kernel/apparmor_restrict_unprivileged_userns", "1"),
    ];
    assert!(!namespaces_allowed(system(&ubuntu), entry(false)));
    let ubuntu_off = [
        ("/proc/sys/user/max_user_namespaces", "63000"),
        ("/proc/sys/kernel/apparmor_restrict_unprivileged_userns", "0"),
    ];
    assert!(namespaces_allowed(system(&ubuntu_off), entry(false)));
    assert!(!namespaces_allowed(
        system(&[("/proc/sys/user/max_user_namespaces", "0")]),
        entry(false)
    ));
    assert!(!namespaces_allowed(system(&[]), entry(false)), "no namespaces at all");
    // No limit told, but the namespaces' entry there: they're made.
    assert!(namespaces_allowed(system(&[]), entry(true)));
}
