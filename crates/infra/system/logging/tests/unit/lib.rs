use super::*;

/// A run's id: 12 hex digits, the same all along.
#[test]
fn a_run_has_one_id() {
    let id = run_id();
    assert_eq!(id.len(), 12);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(run_id(), id);
}

/// Started into a folder: what's logged reaches the day's file once flushed.
#[test]
fn what_s_logged_is_on_disk_once_flushed() {
    use tracing_subscriber::layer::SubscriberExt;
    let dir = std::env::temp_dir().join(format!("logging-lib-{}", uuid::Uuid::new_v4()));
    let options = Options { dir: Some(dir.clone()), console: false, limits: Limits::default() };
    let layer = start(&options).expect("the writer");
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || tracing::info!(answer = 42, "kept"));
    flush();
    let text: String = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    let line: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!((line["msg"].as_str(), line["fields"]["answer"].as_i64()), (Some("kept"), Some(42)));
    assert_eq!(line["run"], run_id());
    let _ = std::fs::remove_dir_all(dir);
}

/// A filter as `RUST_LOG` writes it is read; a wrong one says why.
#[test]
fn a_filter_is_checked_before_it_s_used() {
    assert!(check_filter("info,database_explorer=debug,wgpu_core=warn").is_ok());
    assert!(check_filter("off").is_ok());
    assert!(check_filter("database=loud").is_err());
    assert!(set_filter("info").is_err(), "nothing installed in this test process");
}
