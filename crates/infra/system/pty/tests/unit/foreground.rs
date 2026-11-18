use super::*;

/// A program's start is asked once per process; its name again only once the last one is
/// older than `KEPT` (a forked child bears the shell's name until it `exec`s).
#[cfg(unix)]
#[test]
fn a_known_program_s_start_is_asked_once_and_its_name_at_most_every_half_second() {
    let mut known = Known::default();
    let (mut ages, mut names) = (0, 0);
    let now = Instant::now();
    let mut look = |known: &mut Known, pid, at, name: &str| {
        let (age, seen) = known.look(
            pid,
            at,
            |_| {
                ages += 1;
                Some(Duration::from_secs(5))
            },
            |_| {
                names += 1;
                Some(name.to_owned())
            },
        );
        (age, seen.clone())
    };
    let (first, name) = look(&mut known, 7, now, "zsh");
    assert_eq!((first, name.as_deref()), (Some(Duration::from_secs(5)), Some("zsh")));
    // Within the half second: the same name, the age grown.
    let (again, name) = look(&mut known, 7, now + Duration::from_millis(100), "vim");
    assert_eq!(again, Some(Duration::from_millis(5100)));
    assert_eq!(name.as_deref(), Some("zsh"));
    // Later: its name read again (it `exec`ed).
    let (_, name) = look(&mut known, 7, now + KEPT, "vim");
    assert_eq!(name.as_deref(), Some("vim"));
    // Another process: both asked at once.
    let (_, name) = look(&mut known, 8, now + KEPT, "less");
    assert_eq!(name.as_deref(), Some("less"));
    drop(look);
    assert_eq!((ages, names), (2, 3));
}

/// A program just started may not have `exec`ed yet (the shell's name): its name is asked
/// every time until it has run for `KEPT`.
#[cfg(unix)]
#[test]
fn a_young_program_s_name_is_asked_every_time() {
    let mut known = Known::default();
    let now = Instant::now();
    let young = |_| Some(Duration::from_millis(10));
    let (_, name) = known.look(9, now, young, |_| Some("zsh".into()));
    assert_eq!(name.as_deref(), Some("zsh"));
    let (_, name) = known.look(9, now + Duration::from_millis(1), young, |_| Some("sleep".into()));
    assert_eq!(name.as_deref(), Some("sleep"));
    // Old enough (and named less than `KEPT` ago): kept.
    let (_, name) = known.look(9, now + KEPT, young, |_| Some("x".into()));
    assert_eq!(name.as_deref(), Some("sleep"));
}

/// One look answers both who has the terminal and its name, the system asked again only
/// after `KEPT` (Windows' look at every process costs milliseconds; it was asked for each
/// on every busy frame).
#[test]
fn one_look_answers_the_foreground_and_its_name_for_half_a_second() {
    let mut kept = Kept::default();
    let mut seen = Seen::default();
    let mut asked = 0;
    let now = Instant::now();
    let program = |asked: &mut i32, name: &str| {
        *asked += 1;
        Seen { foreground: Foreground::Program(Duration::from_secs(1)), name: Some(name.into()) }
    };
    kept.look(now, &mut seen, || program(&mut asked, "node.exe"));
    assert_eq!(seen.name.as_deref(), Some("node.exe"));
    for ms in [0, 100, 499] {
        kept.look(now + Duration::from_millis(ms), &mut seen, || program(&mut asked, "other"));
        assert_eq!(seen.name.as_deref(), Some("node.exe"));
        assert_eq!(
            seen.foreground,
            Foreground::Program(Duration::from_secs(1) + Duration::from_millis(ms))
        );
    }
    assert_eq!(asked, 1);
    kept.look(now + KEPT, &mut seen, || program(&mut asked, "other"));
    assert_eq!((asked, seen.name.as_deref()), (2, Some("other")));
}

/// The probe asks its look once for the foreground and once for the name: no more.
#[test]
fn the_probe_answers_both_from_its_look() {
    let looks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&looks);
    let mut probe = Probe::looking(move |seen| {
        counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *seen = Seen { foreground: Foreground::Shell, name: Some("sh".into()) };
    });
    assert_eq!(probe.foreground(), Foreground::Shell);
    assert_eq!(probe.program().as_deref(), Some("sh"));
    assert_eq!(looks.load(std::sync::atomic::Ordering::Relaxed), 2);
}

/// This test's own process: running for a while (since the test binary started), or just
/// now — never in the future.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_process_s_age() {
    let age = age_of(std::process::id()).expect("the system says when a process started");
    assert!(age < Duration::from_secs(24 * 3600), "{age:?}");
    assert_eq!(age_of(u32::MAX >> 1), None);
}
