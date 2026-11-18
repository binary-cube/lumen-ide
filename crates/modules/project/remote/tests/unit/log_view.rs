use super::*;

#[test]
fn times_read_well() {
    assert_eq!(clock(3600 + 61), "01:01:01");
}

/// The log redraws when its project's transfers say something changed — not at every wake.
#[test]
fn the_log_redraws_when_a_transfer_is_heard() {
    let events = kernel::EventBus::new();
    let mut log = TransferLog::default();
    log.set_data(&LogData { project: kernel::ProjectId::new(), events: events.clone() });
    assert!(!log.poll(), "nothing heard");
    events.publish(&remote::events::TransferFinished {
        path: "a.txt".into(),
        action: Action::Upload,
        bytes: 3,
        error: None,
    });
    assert!(log.poll());
    assert!(!log.poll(), "heard once");
    events.publish(&remote::events::TransfersChanged { running: 1, to_upload: 0 });
    assert!(log.poll());
}
