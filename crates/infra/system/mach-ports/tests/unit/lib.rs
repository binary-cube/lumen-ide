#![allow(unsafe_code)]

use std::time::Duration;

use super::*;

unsafe extern "C" {
    static mach_task_self_: u32;
    fn mach_port_allocate(task: u32, right: u32, name: *mut u32) -> i32;
    fn mach_port_insert_right(task: u32, name: u32, right: u32, kind: u32) -> i32;
}

/// A fresh port with a send right of our own (to pass along).
fn port_with_send_right() -> Right {
    let mut name = 0;
    // SAFETY: a new receive right in our task, then a send right made from it.
    unsafe {
        let task = mach_task_self_;
        assert_eq!(mach_port_allocate(task, 1, &raw mut name), 0);
        assert_eq!(mach_port_insert_right(task, name, name, 20), 0);
    }
    Right::adopt(name)
}

/// A right sent through a checked-in service arrives with its numbers when it comes from
/// the expected process; nothing comes in time: none; a sender not expected: dropped.
#[test]
fn a_right_crosses_from_the_expected_sender_only() {
    let name = format!("mach-ports-test.{}.{:x}", std::process::id(), 0x5eed_u32);
    let inbox = Inbox::check_in(&name).expect("checked in");
    let outbox = Outbox::look_up(inbox.name()).expect("looked up");
    let me = Peer { pid: std::process::id(), pid_version: None };
    let nothing = inbox.receive(me, Some(Duration::from_millis(20))).unwrap();
    assert!(nothing.is_none(), "nothing sent yet");

    outbox.send(port_with_send_right(), 7, 42).unwrap();
    let got = inbox.receive(me, Some(Duration::from_secs(2))).unwrap().expect("delivered");
    assert_eq!((got.first, got.second), (7, 42));
    assert_ne!(got.right.port(), 0);

    // From "another" process (we expect a pid that isn't ours): dropped.
    outbox.send(port_with_send_right(), 1, 1).unwrap();
    let other = Peer { pid: me.pid + 1, ..me };
    assert!(inbox.receive(other, Some(Duration::from_millis(100))).unwrap().is_none());
    // Our id but another process's version of it: dropped too.
    outbox.send(port_with_send_right(), 1, 1).unwrap();
    let reused = Peer { pid_version: Some(u32::MAX), ..me };
    assert!(inbox.receive(reused, Some(Duration::from_millis(100))).unwrap().is_none());

    // The inbox gone, its receive right with it: nothing can be sent to it any more (the
    // service's name, if still found, names a dead port).
    let name = inbox.name().to_owned();
    drop(inbox);
    let dead = Outbox::look_up(&name).map(|o| o.send(port_with_send_right(), 0, 0));
    assert!(!matches!(dead, Ok(Ok(()))), "nothing reaches a gone inbox");
}

/// Messages dropped one after another never stretch the wait: it ends at its deadline.
#[test]
fn dropped_messages_dont_stretch_the_wait() {
    let name = format!("mach-ports-flood.{}.{:x}", std::process::id(), 0xf100d_u32);
    let inbox = Inbox::check_in(&name).expect("checked in");
    let outbox = Outbox::look_up(inbox.name()).expect("looked up");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flooding = std::sync::Arc::clone(&stop);
    let flood = std::thread::spawn(move || {
        while !flooding.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = outbox.send(port_with_send_right(), 0, 0);
        }
    });
    let stranger = Peer { pid: std::process::id() + 1, pid_version: None };
    let started = std::time::Instant::now();
    assert!(inbox.receive(stranger, Some(Duration::from_millis(200))).unwrap().is_none());
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    flood.join().unwrap();
}

/// A receive waiting for good is ended by a wake from another thread; wakes sent before it
/// took one are one, and a later wake still wakes; a port sent meanwhile still comes.
#[test]
fn a_receive_waiting_for_good_is_woken() {
    let name = format!("mach-ports-wake.{}.{:x}", std::process::id(), 0x3a4e_u32);
    let inbox = Inbox::check_in(&name).expect("checked in");
    let waker = inbox.waker().expect("a waker");
    let me = Peer { pid: std::process::id(), pid_version: None };
    let woken = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        waker.wake();
        waker.wake();
        waker
    });
    let started = std::time::Instant::now();
    assert!(inbox.receive(me, None).unwrap().is_none(), "woken, nothing delivered");
    assert!(started.elapsed() < Duration::from_secs(5));
    let waker = woken.join().unwrap();
    let since = std::time::Instant::now();
    assert!(inbox.receive(me, Some(Duration::from_millis(300))).unwrap().is_none());
    assert!(since.elapsed() >= Duration::from_millis(250), "the two wakes were one");
    waker.wake();
    assert!(inbox.receive(me, Some(Duration::from_secs(2))).unwrap().is_none(), "woken again");
    let outbox = Outbox::look_up(inbox.name()).expect("looked up");
    outbox.send(port_with_send_right(), 3, 4).unwrap();
    let got = inbox.receive(me, None).unwrap().expect("delivered");
    assert_eq!((got.first, got.second), (3, 4));
}

/// The audit token's words name the process and its id's version.
#[test]
fn a_peer_is_read_from_its_audit_token() {
    let token = [0, 0, 0, 0, 0, 42, 0, 7];
    assert_eq!(Peer::from_audit_token(&token), Peer { pid: 42, pid_version: Some(7) });
}
