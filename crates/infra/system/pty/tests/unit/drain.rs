use super::*;

/// A drain with no handle starts nothing and stops at once.
#[test]
fn a_drain_without_a_handle_does_nothing() {
    let mut drain = Drain::default();
    drain.start();
    drop(drain);
}

/// Wanted only while its program runs; a drain with nobody to ask never is.
#[test]
fn a_drain_is_wanted_only_while_the_program_runs() {
    assert!(!Drain::default().wanted());
    let ended = Arc::new(AtomicBool::new(false));
    let drain = {
        let ended = Arc::clone(&ended);
        Drain::new(Box::new(move || !ended.load(Ordering::Acquire)))
    };
    assert!(drain.wanted());
    ended.store(true, Ordering::Release);
    assert!(!drain.wanted());
}

/// The thread reads what's left (more than a pipe holds: written only because it's read)
/// and ends by itself when the other side is gone (end of file).
#[cfg(unix)]
#[test]
fn a_drain_reads_until_the_other_side_is_gone() {
    use std::io::Write as _;
    use std::os::fd::OwnedFd;
    let (reader, mut writer) = std::io::pipe().unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let fd = OwnedFd::from(reader);
    let thread = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || drain(&fd, &done))
    };
    // Written on a thread: a drain that stops early fails the test, never hangs it.
    let writing = std::thread::spawn(move || {
        let chunk = vec![b'x'; 64 * 1024];
        for _ in 0..16 {
            writer.write_all(&chunk).unwrap();
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !(writing.is_finished() && thread.is_finished()) {
        assert!(Instant::now() < deadline, "the drain didn't read it all, or didn't end");
        std::thread::sleep(Duration::from_millis(10));
    }
    writing.join().unwrap();
    thread.join().unwrap();
}

/// Told the drop is done, the thread stops though the other side is still there.
#[cfg(unix)]
#[test]
fn a_drain_stops_when_told() {
    use std::os::fd::OwnedFd;
    let (reader, _writer) = std::io::pipe().unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let fd = OwnedFd::from(reader);
    let thread = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || drain(&fd, &done))
    };
    done.store(true, Ordering::Release);
    thread.join().unwrap();
}

/// A reader as the emulator's buffered one: what it was given, then `Ok(0)` while empty
/// (it never says it would wait); given more after so many empty looks.
struct Buffered {
    left: usize,
    /// The bytes it's given again after the empty looks counted down (once).
    later: Option<(u32, usize)>,
}

impl Read for Buffered {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            match &mut self.later {
                Some((0, more)) => {
                    self.left = *more;
                    self.later = None;
                }
                Some((looks, _)) => {
                    *looks -= 1;
                    return Ok(0);
                }
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.left);
        self.left -= n;
        Ok(n)
    }
}

/// What a reader holds is read up to where it's empty; one that never is, up to the most,
/// never without end.
#[test]
fn what_is_left_is_read_up_to_empty_or_the_most() {
    assert_eq!(drain_left(&mut Buffered { left: 100_000, later: None }), 100_000);
    assert_eq!(drain_left(&mut Buffered { left: 0, later: None }), 0);
    struct Endless;
    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
    }
    assert_eq!(drain_left(&mut Endless), LEFT_MOST);
}

/// Emptied until it stays empty: what comes after a short pause is read too; what never
/// stops is left at the time given.
#[test]
fn a_reader_is_emptied_until_it_stays_quiet() {
    let mut pausing = Buffered { left: 5_000, later: Some((2, 7_000)) };
    assert_eq!(drain_until_quiet(&mut pausing, Duration::from_secs(5)), 12_000);
    struct Endless;
    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
    }
    let started = Instant::now();
    drain_until_quiet(&mut Endless, Duration::from_millis(50));
    assert!(started.elapsed() < Duration::from_secs(2), "it stopped at the time given");
}
