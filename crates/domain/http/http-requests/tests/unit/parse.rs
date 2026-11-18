//! A `.http` file read in the syntax other HTTP clients write too.

use super::*;

#[test]
fn requests_are_split_named_and_read_with_their_headers_and_body() {
    let text = "\
@host = api.example.com

### Log in
# @name login
POST https://{{host}}/login HTTP/1.1
Content-Type: application/json
# a note between the headers
Accept: application/json

{
  \"user\": \"ana\"
}

###
GET https://{{host}}/users
    ?page=2
    &size=10
Authorization: Bearer {{login.response.body.$.token}}
";
    let file = parse(text);
    assert_eq!(file.variables.len(), 1);
    assert_eq!((&*file.variables[0].name, &*file.variables[0].value), ("host", "api.example.com"));
    assert_eq!(file.requests.len(), 2);
    for request in &file.requests {
        let line = &text[request.line_range.clone()];
        assert!(line.starts_with(&request.method) && !line.ends_with('\n'), "{line:?}");
    }
    assert_eq!(&text[file.requests[1].line_range.clone()], "GET https://{{host}}/users");
    let login = &file.requests[0];
    assert_eq!(login.name.as_deref(), Some("login"), "@name over the title");
    assert_eq!(login.title.as_deref(), Some("Log in"));
    assert_eq!((&*login.method, &*login.url), ("POST", "https://{{host}}/login"));
    assert_eq!(login.version.as_deref(), Some("HTTP/1.1"));
    assert_eq!(login.headers.len(), 2, "the comment isn't a header");
    assert_eq!(login.header("content-type"), Some("application/json"));
    assert_eq!(login.body, [BodyPart::Text("{\n  \"user\": \"ana\"\n}".into())]);
    assert_eq!(login.line, 4);
    let users = &file.requests[1];
    assert_eq!(users.name, None);
    assert_eq!(users.url, "https://{{host}}/users?page=2&size=10", "the URL goes on");
    assert!(users.body.is_empty());
    // Each request's block, for the gutter and Run at the cursor.
    let at = text.find("GET https").unwrap();
    assert_eq!(file.request_at(at).map(|r| r.method.as_str()), Some("GET"));
    let comment = text.find("# @name").unwrap();
    assert_eq!(
        file.request_at(comment).map(|r| r.method.as_str()),
        Some("POST"),
        "its comments too"
    );
    assert_eq!(file.request_at(0), None, "the variables' block holds no request");
    assert_eq!(file.named("login").map(|r| r.line), Some(4));
}

#[test]
fn a_bare_url_is_a_get_and_comments_say_what_to_do() {
    let file = parse(
        "\
// @no-redirect
# @no-cookie-jar
# @no-log
# @timeout 5
# @connection-timeout 500 ms
https://example.com
",
    );
    let request = &file.requests[0];
    assert_eq!((&*request.method, &*request.url), ("GET", "https://example.com"));
    let options = &request.options;
    assert!(options.no_redirect && options.no_cookie_jar && options.no_log);
    assert_eq!(options.timeout, Some(std::time::Duration::from_secs(5)));
    assert_eq!(options.connection_timeout, Some(std::time::Duration::from_millis(500)));
    assert_eq!(request.label(), "GET https://example.com");
}

#[test]
fn files_scripts_and_saving_are_told_apart_from_the_body() {
    let file = parse(
        "\
< {%
  request.variables.set(\"id\", \"7\");
%}
POST https://example.com/upload
Content-Type: multipart/form-data; boundary=WebAppBoundary

--WebAppBoundary
Content-Disposition: form-data; name=\"file\"; filename=\"a.json\"

< ./a.json
--WebAppBoundary--

> {% client.global.set(\"token\", response.body.token); %}
>>! ./last-upload.json
<> 2026-11-11T121924.200.json
",
    );
    let request = &file.requests[0];
    assert_eq!(
        request.pre_script,
        Some(Script::Inline("request.variables.set(\"id\", \"7\");".into()))
    );
    assert_eq!(request.body.len(), 3, "{:?}", request.body);
    assert_eq!(request.body[1], BodyPart::File("./a.json".into()));
    assert!(matches!(&request.body[2], BodyPart::Text(t) if t.trim() == "--WebAppBoundary--"));
    assert_eq!(
        request.post_script,
        Some(Script::Inline("client.global.set(\"token\", response.body.token);".into()))
    );
    assert_eq!(request.save_to, Some(SaveTo { path: "./last-upload.json".into(), replace: true }));
}

#[test]
fn a_handler_file_an_xml_body_and_empty_blocks() {
    let file = parse(
        "\
###
# only a comment here

### XML
POST https://example.com/soap
Content-Type: text/xml

<envelope><body/></envelope>

> ./handler.js
",
    );
    assert_eq!(file.requests.len(), 1, "a block with no request line has none");
    let request = &file.requests[0];
    assert_eq!(request.name.as_deref(), Some("XML"), "the title names it");
    assert_eq!(request.body, [BodyPart::Text("<envelope><body/></envelope>".into())]);
    assert_eq!(request.post_script, Some(Script::File("./handler.js".into())));
}

#[test]
fn durations_take_their_units() {
    use std::time::Duration;
    assert_eq!(duration("10"), Some(Duration::from_secs(10)));
    assert_eq!(duration("250ms"), Some(Duration::from_millis(250)));
    assert_eq!(duration("2 m"), Some(Duration::from_secs(120)));
    assert_eq!(duration("soon"), None);
}

#[test]
fn a_version_with_spaces_stays_out_of_the_url() {
    let file = parse("GET https://e.com/a HTTP/2 (Prior Knowledge)\n");
    let request = &file.requests[0];
    assert_eq!(request.url, "https://e.com/a");
    assert_eq!(request.version.as_deref(), Some("HTTP/2 (Prior Knowledge)"));
    let file = parse("GET https://e.com/HTTP/x HTTP/1.1\n");
    assert_eq!(file.requests[0].url, "https://e.com/HTTP/x", "HTTP/ inside the URL isn't it");
}
