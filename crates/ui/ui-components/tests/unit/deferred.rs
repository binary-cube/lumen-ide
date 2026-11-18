use super::*;

fn ready(deferred: &mut Deferred) -> Option<ViewerAction> {
    for _ in 0..1000 {
        if let Some(action) = deferred.poll() {
            return Some(action);
        }
        if !deferred.is_busy() {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("it never ended");
}

#[test]
fn the_action_comes_once_ready_and_the_last_start_wins() {
    let mut deferred = Deferred::default();
    assert_eq!(deferred.poll(), None);
    deferred.start("test", || Some(ViewerAction::OpenLink("first".into())));
    deferred.start("test", || Some(ViewerAction::OpenLink("second".into())));
    assert_eq!(ready(&mut deferred), Some(ViewerAction::OpenLink("second".into())));
    assert!(!deferred.is_busy());
    deferred.start("test", || None);
    assert_eq!(ready(&mut deferred), None);
}
