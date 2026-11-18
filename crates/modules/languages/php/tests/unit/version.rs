use super::*;

#[test]
fn constraints() {
    assert_eq!(floor("^8.2").as_deref(), Some("8.2"));
    assert_eq!(floor(">=7.4 <8.3").as_deref(), Some("7.4"));
    assert_eq!(floor("~7.4||^8.0").as_deref(), Some("7.4"));
    assert_eq!(floor("^8.1.3").as_deref(), Some("8.1"));
    assert_eq!(floor("8.3.*").as_deref(), Some("8.3"));
    assert_eq!(floor(">=9.0").as_deref(), Some("8.5"));
    assert_eq!(floor("*"), None);
}

#[test]
fn detection_prefers_platform() {
    let read = |_: &std::path::Path| {
        Some(r#"{"require":{"php":"^8.1"},"config":{"platform":{"php":"8.3.4"}}}"#.into())
    };
    let project = Project {
        root: Some(std::path::Path::new("/p")),
        read: &read,
        list: &|_| Vec::new(),
        modified: &|_| None,
        levels: &[],
        languages: &[],
    };
    let detected = from_composer(&project).unwrap();
    assert_eq!(detected.level, "8.3");
    assert!(detected.source.contains("platform"));
}
