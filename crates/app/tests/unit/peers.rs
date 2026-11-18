//! The app's instances of the same settings: found by their channels' names, told what
//! stands for the whole app in one line.

use super::*;
use std::sync::mpsc;

fn relax() -> Over {
    Over { extension: "view-relax".to_owned(), full: None }
}

#[test]
fn a_message_says_a_view_opened_or_closed() {
    assert_eq!(parse(&message(Some(&relax()))), Some(Some(relax())));
    assert_eq!(parse(&message(None)), Some(None));
    assert_eq!(parse("over\r\n"), Some(None));
    assert_eq!(parse("overt x\n"), None, "not ours");
    assert_eq!(parse("over \n"), None);
    assert_eq!(parse("open a\n"), None);
}

static TOLD: Mutex<Option<mpsc::Sender<Option<Over>>>> = Mutex::new(None);

#[test]
fn the_instances_of_a_group_find_and_tell_each_other() {
    let (tx, rx) = mpsc::channel();
    *TOLD.lock().unwrap() = Some(tx);
    let group = format!("test{}", std::process::id());
    let (one, two) = (Peer { group: group.clone(), id: 1 }, Peer { group: group.clone(), id: 2 });
    let elsewhere = Peer { group: format!("{group}x"), id: 3 };
    let heard: fn(Option<Over>) = |said| {
        if let Some(tx) = &*TOLD.lock().unwrap() {
            let _ = tx.send(said);
        }
    };
    listen(&one, heard).unwrap();
    listen(&two, heard).unwrap();
    listen(&elsewhere, heard).unwrap();
    // Each finds the other of its group, not itself, not another group's.
    assert_eq!(others(&one), std::slice::from_ref(&two));
    assert_eq!(others(&two), std::slice::from_ref(&one));
    send_to(&two, &message(Some(&relax())));
    let said = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(said, Some(relax()));
    // One gone (its channel left behind): not found once told.
    #[cfg(not(windows))]
    {
        let gone = Peer { group: one.group.clone(), id: 9 };
        std::fs::write(crate::instance::socket_dir().join(format!("{}.sock", gone.name())), "")
            .unwrap();
        assert!(others(&one).contains(&gone));
        send_to(&gone, &message(None));
        assert!(!others(&one).contains(&gone), "removed once it didn't answer");
        for peer in [&one, &two, &elsewhere] {
            let _ = std::fs::remove_file(
                crate::instance::socket_dir().join(format!("{}.sock", peer.name())),
            );
        }
    }
}
