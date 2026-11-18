use super::*;
use serde_json::json;

fn list() -> Value {
    json!([
        {"tag_name": "v1.1.0", "name": "", "draft": true, "prerelease": false, "body": "draft", "assets": []},
        {"tag_name": "v1.0.0", "name": "Lumen 1.0.0", "draft": false, "prerelease": false,
         "body": "## What's new\n- stable", "published_at": "2026-12-01T10:00:00Z",
         "html_url": "https://github.com/o/r/releases/tag/v1.0.0",
         "assets": [{"name": "latest.json", "browser_download_url": "https://github.com/o/r/releases/download/v1.0.0/latest.json", "size": 700}]},
        {"tag_name": "v1.0.0-beta.2", "name": "Lumen 1.0.0-beta.2", "draft": false, "prerelease": true,
         "body": "beta", "assets": []},
        {"tag_name": "nightly", "name": "Nightly", "draft": false, "prerelease": true, "body": "", "assets": []},
        {"tag_name": "v1.0.0-beta.1", "draft": false, "prerelease": true, "assets": []}
    ])
}

/// The host's list read: drafts and tags that aren't versions left out, a title missing
/// given the tag, the files with their URLs and sizes.
#[test]
fn the_releases_list_is_read() {
    let releases = parse(&list());
    let tags: Vec<&str> = releases.iter().map(|r| r.tag.as_str()).collect();
    assert_eq!(tags, ["v1.0.0", "v1.0.0-beta.2", "v1.0.0-beta.1"]);
    let stable = &releases[0];
    assert_eq!(stable.version.to_string(), "1.0.0");
    assert!(!stable.prerelease);
    assert_eq!(stable.notes, "## What's new\n- stable");
    assert_eq!(stable.asset("latest.json").map(|a| a.size), Some(700));
    assert_eq!(releases[2].name, "v1.0.0-beta.1", "no title: its tag");
    assert!(parse(&json!({"message": "rate limited"})).is_empty());
}

/// The newest above the one running: pre-releases only for whoever takes them, a skipped
/// version never (a newer one again), nothing when it's the newest already.
#[test]
fn the_newest_worth_offering_is_chosen() {
    let releases = parse(&list());
    let at = |text: &str| Version::parse(text).unwrap();
    let offered = |current: &str, pre: bool, skipped: Option<&str>| {
        let skipped = skipped.map(at);
        newest(&releases, &at(current), pre, skipped.as_ref()).map(|r| r.tag.clone())
    };
    assert_eq!(offered("1.0.0-beta.1", true, None).as_deref(), Some("v1.0.0"));
    assert_eq!(offered("1.0.0-beta.1", false, None).as_deref(), Some("v1.0.0"));
    assert_eq!(offered("1.0.0-beta.1", true, Some("1.0.0")).as_deref(), Some("v1.0.0-beta.2"));
    assert_eq!(offered("0.9.0", false, Some("1.0.0")), None, "only pre-releases left");
    assert_eq!(offered("1.0.0", true, None), None, "the newest already");
}
