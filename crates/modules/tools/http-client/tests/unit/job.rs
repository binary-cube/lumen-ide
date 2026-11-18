//! A run's own folder kept out of git, only the newest files kept in it, the cookies two
//! runs change at once both kept; a loop in the graph said, not run.

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("http-job-{name}-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_folder_ignores_itself_and_keeps_only_the_newest() {
    let dir = scratch("folder");
    let folder = dir.join("http");
    assert!(ready_folder(&folder));
    assert_eq!(std::fs::read_to_string(folder.join(".gitignore")).unwrap(), "*\n");
    let bodies = folder.join("bodies");
    std::fs::create_dir_all(&bodies).unwrap();
    for i in 0..KEPT + 3 {
        std::fs::write(bodies.join(format!("{i:03}.body")), b"x").unwrap();
    }
    keep_newest(&bodies);
    assert_eq!(std::fs::read_dir(&bodies).unwrap().count(), KEPT);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn two_runs_cookies_are_both_kept() {
    let dir = scratch("cookies");
    let runs = Runs::of(Some(kernel::ProjectId::new()), Some(&dir));
    let url = http_requests::url::Url::parse("https://e.com/").unwrap();
    let start = CookieJar::default();
    // Both began from the same jar; each set its own.
    let (mut a, mut b) = (start.clone(), start.clone());
    a.store(&url, "a=1", 0);
    b.store(&url, "b=2", 0);
    keep_cookies(&runs, &dir, &start, &a);
    keep_cookies(&runs, &dir, &start, &b);
    let kept = cookies(&runs, &dir);
    let mut names: Vec<_> = kept.cookies().iter().map(|c| c.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b"], "neither undid the other's");
    assert!(dir.join("cookies").exists(), "written for the next session");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_loop_in_the_graph_is_said_on_a_request_of_it() {
    let runs = Runs::of(Some(kernel::ProjectId::new()), None);
    let file = Path::new("/p/loop.http");
    let text = "GET https://e.com/free\n\n###\n# @name b\nGET https://e.com/{{c.response.body.$.x}}\n\n###\n# @name c\nGET https://e.com/{{b.response.body.$.y}}\n";
    assert!(run_graph(&runs, file, text));
    let all = runs.runs();
    assert_eq!(all.len(), 1, "nothing run");
    assert_eq!(all[0].label, "b", "said on the loop, not on the free request");
    assert!(
        matches!(&all[0].state, RunState::Failed(why) if why.contains('b') && why.contains('c'))
    );
}

#[test]
fn a_script_is_read_from_the_file_it_names_beside_the_request() {
    let dir = scratch("script");
    std::fs::write(dir.join("handler.js"), "client.log('hi')").unwrap();
    let file = dir.join("api.http");
    let named = http_requests::Script::File("./handler.js".into());
    assert_eq!(script_source(&file, &named).unwrap(), "client.log('hi')");
    let missing = http_requests::Script::File("nope.js".into());
    assert!(script_source(&file, &missing).unwrap_err().starts_with("nope.js"));
    let inline = http_requests::Script::Inline("1 + 1".into());
    assert_eq!(script_source(&file, &inline).unwrap(), "1 + 1");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_script_sees_the_response_s_body_and_charset() {
    let response = http_requests::transport::Incoming {
        status: 201,
        reason: "Created".into(),
        version: "HTTP/1.1".into(),
        headers: vec![http_requests::Header {
            name: "Content-Type".into(),
            value: "application/json; charset=\"utf-8\"".into(),
        }],
        body: IncomingBody::Memory(br#"{"id": 7}"#.to_vec()),
        url: "https://e.com".into(),
        hops: Vec::new(),
        timings: http_requests::transport::Timings::default(),
    };
    let seen = script_response(&response);
    assert_eq!((seen.status, seen.body.as_str()), (201, r#"{"id": 7}"#));
    assert_eq!(seen.content_type.as_deref(), Some("application/json"));
    assert_eq!(seen.charset.as_deref(), Some("utf-8"));
}

#[test]
fn a_response_is_saved_only_inside_the_project() {
    let root = std::env::temp_dir().join(format!("http-confined-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(root.join("api")).unwrap();
    assert_eq!(confined(&root.join("api/../out/a.json"), &root), Some(root.join("out/a.json")));
    assert_eq!(confined(&root.join("api/../../etc/a.json"), &root), None);
    assert_eq!(confined(Path::new("/etc/passwd"), &root), None);
    let _ = std::fs::remove_dir_all(root);
}
