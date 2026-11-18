use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn ready_at_the_first_frame_shows_no_splash() {
    let start = Instant::now();
    let mut clock = SplashClock::new(true);
    assert_eq!(clock.frame(start, true), (SplashShow::Workbench, Some(false)));
    assert!(clock.is_gone());
    assert_eq!(clock.frame(start + ms(16), true), (SplashShow::Workbench, None));
}

#[test]
fn ready_within_the_grace_shows_only_the_background_then_the_workbench() {
    let start = Instant::now();
    let mut clock = SplashClock::new(true);
    assert_eq!(clock.frame(start, false), (SplashShow::Background, None));
    assert_eq!(clock.frame(start + ms(60), false), (SplashShow::Background, None));
    // Ready before the grace ends: the splash never shows.
    assert_eq!(clock.frame(start + ms(100), true), (SplashShow::Workbench, Some(false)));
    assert!(clock.is_gone());
}

#[test]
fn ready_late_keeps_the_splash_until_ready_then_fades() {
    let start = Instant::now();
    let mut clock = SplashClock::new(true);
    assert_eq!(clock.frame(start, false).0, SplashShow::Background);
    // The grace is over: the splash shows (alone, its phase from now).
    let shown = start + GRACE;
    assert_eq!(
        clock.frame(shown, false).0,
        SplashShow::Splash { elapsed: Duration::ZERO, opacity: 1.0 }
    );
    // Still not ready, long past its minimum: it stays.
    let later = shown + ms(900);
    assert_eq!(clock.frame(later, false).0, SplashShow::Splash { elapsed: ms(900), opacity: 1.0 });
    // Ready: it fades out over the workbench, then it's gone.
    let ready = later + ms(10);
    assert_eq!(clock.frame(ready, true).0, SplashShow::Splash { elapsed: ms(910), opacity: 1.0 });
    match clock.frame(ready + FADE / 2, true).0 {
        SplashShow::Splash { opacity, .. } => assert!((opacity - 0.5).abs() < 0.01, "{opacity}"),
        other => panic!("fading expected, got {other:?}"),
    }
    assert_eq!(clock.frame(ready + FADE, true), (SplashShow::Workbench, Some(true)));
    assert!(clock.is_gone());
}

#[test]
fn ready_just_after_the_grace_still_shows_the_minimum() {
    let start = Instant::now();
    let mut clock = SplashClock::new(true);
    clock.frame(start, false);
    let shown = start + GRACE;
    clock.frame(shown, false);
    // Ready at once: it doesn't flash away, it stays its minimum.
    assert_eq!(
        clock.frame(shown + ms(20), true).0,
        SplashShow::Splash { elapsed: ms(20), opacity: 1.0 }
    );
    assert_eq!(
        clock.frame(shown + MIN_SHOWN - ms(1), true).0,
        SplashShow::Splash { elapsed: MIN_SHOWN - ms(1), opacity: 1.0 }
    );
    // Its minimum reached: the fade starts.
    assert_eq!(
        clock.frame(shown + MIN_SHOWN, true).0,
        SplashShow::Splash { elapsed: MIN_SHOWN, opacity: 1.0 }
    );
    assert_eq!(clock.frame(shown + MIN_SHOWN + FADE, true).0, SplashShow::Workbench);
}

#[test]
fn never_ready_ends_after_the_most_it_stays() {
    let start = Instant::now();
    let mut clock = SplashClock::new(true);
    clock.frame(start, false);
    let shown = start + GRACE;
    clock.frame(shown, false);
    assert!(matches!(
        clock.frame(shown + MAX_SHOWN - ms(1), false).0,
        SplashShow::Splash { opacity, .. } if opacity == 1.0
    ));
    clock.frame(shown + MAX_SHOWN, false);
    assert_eq!(clock.frame(shown + MAX_SHOWN + FADE, false), (SplashShow::Workbench, Some(true)));
}

#[test]
fn disabled_never_shows_it() {
    let start = Instant::now();
    let mut clock = SplashClock::new(false);
    assert!(clock.is_gone());
    assert_eq!(clock.frame(start, false), (SplashShow::Workbench, None));
    assert_eq!(clock.frame(start + ms(500), false), (SplashShow::Workbench, None));
}
