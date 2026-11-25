use std::process::{Child, Command};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::*;

/// The graces shortened for the tests: the hangup's, then the stop's.
const SHORT: (Duration, Duration) = (Duration::from_millis(300), Duration::from_millis(500));

/// Born around now (the program just started).
fn just_now() -> Born {
    let now = now_secs();
    Born { after: now.saturating_sub(5), before: now + 5 }
}

/// A shell running `script`, its id, and a thread reaping it (as the PTY's reader does):
/// `true` once it ended within the limit (else it's ended here).
fn started(script: &str) -> (u32, JoinHandle<bool>) {
    let mut child: Child = Command::new("sh").args(["-c", script]).spawn().unwrap();
    let pid = child.id();
    let reaper = std::thread::spawn(move || {
        let since = Instant::now();
        while since.elapsed() < Duration::from_secs(10) {
            if child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        false
    });
    // Its trap set before anything is asked of it.
    std::thread::sleep(Duration::from_millis(200));
    (pid, reaper)
}

fn hang_up(pid: u32) {
    // SAFETY: a signal to a child this test just started.
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGHUP);
    }
}

#[test]
fn a_program_going_on_after_its_hangup_is_stopped() {
    let (pid, reaper) = started("trap '' HUP; sleep 30");
    hang_up(pid);
    assert!(end_after(pid, just_now(), SHORT), "it had to be stopped");
    assert!(reaper.join().unwrap(), "it ended");
}

#[test]
fn a_program_ending_on_its_hangup_is_left_alone() {
    let (pid, reaper) = started("exec sleep 30");
    hang_up(pid);
    assert!(!end_after(pid, just_now(), SHORT));
    assert!(reaper.join().unwrap());
}

/// A program under the id that started before the session, or after it (the id reused once
/// the session's program ended), is another one: untouched.
#[test]
fn a_program_born_outside_the_session_s_start_is_never_touched() {
    let (pid, reaper) = started("trap '' HUP; exec sleep 2");
    let now = now_secs();
    assert!(!end_after(pid, Born { after: now + 60, before: now + 61 }, SHORT), "older");
    assert!(!end_after(pid, Born { after: now - 120, before: now - 60 }, SHORT), "newer");
    // It ends by itself, at its own time.
    assert!(reaper.join().unwrap());
}
