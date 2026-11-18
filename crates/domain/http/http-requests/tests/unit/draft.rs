//! A request read into the editor's form and written back: what it holds, round trips,
//! what's disabled, the authentication and the body by kind.

use super::*;

/// The draft of `text`'s first request.
fn draft(text: &str) -> Draft {
    let file = crate::parse(text);
    let request = &file.requests[0];
    Draft::of(request, text)
}

#[test]
fn a_request_is_read_into_its_parts() {
    let text = "### List users\n# Lists the users.\n# Paged.\n# @name list\n# @no-log\n# @disabled-param debug=1\n# @disabled X-Trace: on\nGET https://e.com/users?page=2&size=10\nAccept: application/json\nAuthorization: Bearer {{token}}\n";
    let d = draft(text);
    assert_eq!((d.title.as_str(), d.name.as_deref()), ("List users", Some("list")));
    assert_eq!(d.docs, "Lists the users.\nPaged.");
    assert_eq!(d.url, "https://e.com/users");
    assert_eq!(d.params.len(), 3);
    assert_eq!((d.params[0].name.as_str(), d.params[0].value.as_str()), ("page", "2"));
    assert!(!d.params[2].enabled && d.params[2].name == "debug");
    assert_eq!(d.headers.len(), 2, "Accept and the disabled one; Authorization is the auth");
    assert!(!d.headers[1].enabled);
    assert_eq!(d.auth, Auth::Bearer { token: "{{token}}".into() });
    assert_eq!(d.directives, ["@no-log"]);
}

#[test]
fn what_is_written_reads_back_the_same() {
    let cases = [
        "### A\nGET https://e.com/a?x=1\nAccept: */*\n",
        "### Log in\n# @name login\nPOST https://e.com/login\nContent-Type: application/json\n\n{\"user\": \"ana\"}\n\n> {%\n  client.global.set(\"t\", response.body.t);\n%}\n",
        "### Form\nPOST https://e.com/f\nContent-Type: application/x-www-form-urlencoded\n\na=1&b=2\n",
        "### Key\n# @auth api-key X-API-Key\nGET https://e.com/k\nX-API-Key: {{key}}\n",
        "### Basic\nGET https://e.com/b\nAuthorization: Basic ana p4ss\n",
        "### OAuth\nGET https://e.com/o\nAuthorization: Bearer {{$auth.token(\"main\")}}\n",
        "### File\nPOST https://e.com/u\nContent-Type: application/octet-stream\n\n< ./data.bin\n",
    ];
    for text in cases {
        let first = draft(text);
        let written = first.to_text();
        let again = draft(&written);
        assert_eq!(first, again, "{text}\n---\n{written}");
    }
    let key = draft(cases[3]);
    assert_eq!(
        key.auth,
        Auth::ApiKey { name: "X-API-Key".into(), value: "{{key}}".into(), in_query: false }
    );
    assert_eq!(draft(cases[5]).auth, Auth::OAuth2 { id: "main".into() });
    assert_eq!(
        draft(cases[2]).body,
        Body::UrlEncoded(vec![Field::on("a", "1"), Field::on("b", "2")])
    );
}

#[test]
fn a_new_request_is_written_as_phpstorm_reads_it() {
    let mut d = Draft::new("Create");
    d.method = "POST".into();
    d.url = "{{host}}/users".into();
    d.params = vec![Field::on("dry", "1"), Field { enabled: false, ..Field::on("x", "2") }];
    d.headers = vec![Field::on("Accept", "application/json")];
    d.auth = Auth::Basic { user: "ana".into(), password: "{{password}}".into() };
    d.body = Body::Json("{\"name\": \"ana\"}".into());
    d.docs = "Creates a user.".into();
    let text = d.to_text();
    let file = crate::parse(&text);
    let request = &file.requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "{{host}}/users?dry=1");
    assert!(
        request.headers.iter().any(|h| h.name == "Content-Type" && h.value == "application/json")
    );
    assert!(
        request
            .headers
            .iter()
            .any(|h| h.name == "Authorization" && h.value == "Basic ana {{password}}")
    );
    assert!(text.contains("# Creates a user.\n") && text.contains("# @disabled-param x=2\n"));
    assert_eq!(draft(&text), d, "and back");
}

#[test]
fn multipart_and_graphql_bodies() {
    let mut d = Draft::new("Upload");
    d.method = "POST".into();
    d.url = "https://e.com/up".into();
    d.body = Body::Multipart(vec![
        Part { name: "title".into(), value: "A cat".into(), file: false },
        Part { name: "photo".into(), value: "./cat.png".into(), file: true },
    ]);
    let text = d.to_text();
    assert!(text.contains("filename=\"cat.png\"\n\n< ./cat.png\n"), "{text}");
    assert_eq!(draft(&text).body, d.body);
    let mut g = Draft::new("Q");
    g.url = "https://e.com/graphql".into();
    g.body = Body::GraphQl { query: "{ me { id } }".into(), variables: "{\"a\": 1}".into() };
    let text = g.to_text();
    assert!(text.contains("GRAPHQL https://e.com/graphql"), "GraphQL's own method: {text}");
    assert_eq!(draft(&text).body, g.body);
    assert_eq!(
        split_query("https://e.com/a?b=1&c"),
        ("https://e.com/a".into(), vec![Field::on("b", "1"), Field::on("c", "")])
    );
}

#[test]
fn a_request_is_written_back_in_its_place_and_a_new_one_at_the_end() {
    let text = "@host = e.com\n\n### One\nGET https://{{host}}/1\n\n### Two\nGET https://{{host}}/2\n\n### Three\nGET https://{{host}}/3\n";
    let file = crate::parse(text);
    let span = block_of(text, &file.requests[1]);
    assert!(text[span.clone()].starts_with("### Two\n"), "{:?}", &text[span]);
    let mut two = draft(&text[block_of(text, &file.requests[1])]);
    two.url = "https://{{host}}/two".into();
    let written = write_into(text, Some(1), &two);
    let again = crate::parse(&written);
    assert_eq!(again.requests.len(), 3);
    assert_eq!(again.requests[1].url, "https://{{host}}/two");
    assert_eq!(again.requests[0].url, "https://{{host}}/1");
    assert_eq!(again.requests[2].url, "https://{{host}}/3");
    assert_eq!(again.variables.len(), 1, "the file's variables kept");
    let added = write_into(&written, None, &Draft::new("Four"));
    assert_eq!(crate::parse(&added).requests.len(), 4);
}

#[test]
fn a_request_is_removed_from_its_file() {
    let text = "### One\nGET https://e.com/1\n\n### Two\nGET https://e.com/2\n\n### Three\nGET https://e.com/3\n";
    let left = remove_from(text, 1);
    let urls: Vec<String> = crate::parse(&left).requests.into_iter().map(|r| r.url).collect();
    assert_eq!(urls, ["https://e.com/1", "https://e.com/3"]);
    assert_eq!(remove_from(text, 9), text, "none such");
}

#[test]
fn a_block_is_found_again_wherever_it_went_and_not_once_changed() {
    let text = "### One\nGET https://e.com/1\n\n### Two\nGET https://e.com/2\n";
    let file = crate::parse(text);
    let two = text[block_of(text, &file.requests[1])].to_owned();
    assert_eq!(find_block(text, &two), Some(1));
    let moved = format!("### Zero\nGET https://e.com/0\n\n{text}");
    assert_eq!(find_block(&moved, &two), Some(2), "found where it is now");
    let changed = text.replace("/2", "/22");
    assert_eq!(find_block(&changed, &two), None, "changed elsewhere: not written over");
}

#[test]
fn raw_blocks_are_duplicated_and_moved_as_they_are() {
    let text = "### One\n# @name one\n# @no-log\nGET https://e.com/1 HTTP/2\n\n> ./handler.js\n>> out.json\n\n### Two\nGET https://e.com/2\n";
    let hash = block_hash(&text[block_of(text, &crate::parse(text).requests[0])]);
    assert_eq!(find_by_hash(text, hash), Some(0));
    assert_eq!(find_by_hash(&text.replace("/1", "/x"), hash), None, "changed: not it");
    let doubled = duplicate_raw(text, 0, "copy").unwrap();
    let parsed = crate::parse(&doubled);
    assert_eq!(parsed.requests.len(), 3);
    let copy = &parsed.requests[2];
    assert_eq!(copy.title.as_deref(), Some("One (copy)"));
    assert_eq!(copy.name.as_deref(), Some("One (copy)"), "no second `one`");
    assert!(copy.options.no_log && copy.version.as_deref() == Some("HTTP/2"));
    assert!(
        matches!(copy.post_script, Some(crate::Script::File(_))) && copy.save_to.is_some(),
        "nothing lost"
    );
    let (left, into) = move_raw(text, 0, "### Other\nGET https://e.com/o\n").unwrap();
    assert_eq!(crate::parse(&left).requests.len(), 1);
    let moved = crate::parse(&into);
    assert_eq!(moved.requests.len(), 2);
    assert_eq!(moved.requests[1].name.as_deref(), Some("one"), "moved with its name");
    assert!(moved.requests[1].save_to.is_some());
}

#[test]
fn what_the_editor_doesnt_show_is_written_back_as_it_was() {
    let text = "### Upload\n# Sends it.\n< ./pre.js\nPOST https://e.com/up HTTP/2\nContent-Type: multipart/form-data; boundary=XYZ\n\n--XYZ\nContent-Disposition: form-data; name=\"doc\"; filename=\"a.pdf\"\nContent-Type: application/pdf\n\n< ./files/a.pdf\n--XYZ--\n\n> ./post.js\n>>! ./out/up.json\n";
    let d = draft(text);
    assert_eq!(d.docs, "Sends it.");
    assert_eq!(
        (d.pre_script.as_deref(), d.post_script.as_deref()),
        (None, None),
        "files, not text"
    );
    let written = d.to_text();
    let again = &crate::parse(&written).requests[0];
    assert_eq!(again.version.as_deref(), Some("HTTP/2"));
    assert_eq!(again.pre_script, Some(crate::Script::File("./pre.js".into())));
    assert_eq!(again.post_script, Some(crate::Script::File("./post.js".into())));
    assert_eq!(
        again.save_to.as_ref().map(|s| (s.path.as_str(), s.replace)),
        Some(("./out/up.json", true))
    );
    assert!(
        written.contains("boundary=XYZ")
            && written.contains("filename=\"a.pdf\"")
            && written.contains("Content-Type: application/pdf"),
        "{written}"
    );
    // A field changed: written from the fields.
    let mut changed = d;
    if let Body::Multipart(parts) = &mut changed.body {
        parts[0].name = "file".into();
    }
    assert!(changed.to_text().contains("boundary=WebAppBoundary"));
}

#[test]
fn a_pre_script_s_comments_are_not_the_request_s_description() {
    let text = "### A\n# Mine.\n< {%\n  // the script's\n  request.variables.set(\"a\", 1);\n%}\nGET https://e.com\n";
    assert_eq!(draft(text).docs, "Mine.");
}
