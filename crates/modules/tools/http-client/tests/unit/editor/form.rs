//! The editor's form made from a draft and back; the URL and its parameters kept together;
//! the headers the app adds itself.

use super::*;

fn draft_of(text: &str) -> Draft {
    let file = http_requests::parse(text);
    Draft::of(&file.requests[0], text)
}

#[test]
fn a_draft_makes_a_form_that_makes_it_again() {
    let text = "### Create\n# Makes one.\n# @disabled X-Off: 1\nPOST https://e.com/users?dry=1\nAccept: application/json\nAuthorization: Basic ana p4ss\nContent-Type: application/json\n\n{\"name\": \"ana\"}\n";
    let draft = draft_of(text);
    let form = Form::of(&draft);
    assert_eq!(form.url.text(), "https://e.com/users?dry=1");
    assert_eq!(form.auth_kind, AuthKind::Basic);
    assert!(form.auth[1].is_masked(), "a password hidden");
    assert_eq!(form.body_kind, BodyKind::Json);
    assert_eq!(form.draft(), draft);
}

#[test]
fn the_url_and_its_parameters_follow_each_other() {
    let mut form = Form::of(&draft_of("### A\nGET https://e.com/a?x=1\n"));
    form.url.set_text("https://e.com/a?x=1&y=2");
    form.url_typed();
    assert_eq!(form.params.len(), 2);
    form.params[0].enabled = false;
    form.params_changed();
    assert_eq!(form.url.text(), "https://e.com/a?y=2", "a parameter turned off leaves the URL");
    form.url_typed();
    assert_eq!(form.params.len(), 2, "the disabled one kept");
    assert!(!form.params.last().unwrap().enabled);
}

#[test]
fn the_app_s_own_headers_are_shown_as_its() {
    let mut form = Form::of(&draft_of("### A\nPOST https://e.com/a\n"));
    form.set_body_kind(BodyKind::Json);
    let auto: Vec<String> = form.automatic_headers().into_iter().map(|(n, _)| n).collect();
    assert_eq!(auto, ["Content-Type", "Host", "User-Agent"]);
}

/// Each body kind's `Content-Type`, told without the draft (its body copied), is the
/// draft's.
#[test]
fn a_body_s_content_type_is_told_without_copying_it() {
    let mut form = Form::of(&draft_of("### A\nPOST https://e.com/a\n"));
    for kind in BodyKind::ALL {
        form.set_body_kind(kind);
        assert_eq!(form.body_content_type(), form.draft().body.content_type(), "{kind:?}");
    }
}

#[test]
fn its_settings_are_the_request_s_directives() {
    let text = "### A\n# @no-redirect\n# @timeout 5\n# @connection-timeout 2 m\n# @custom x\nGET https://e.com/a\n";
    let draft = draft_of(text);
    let mut form = Form::of(&draft);
    assert!(!form.follow_redirects && form.cookie_jar && form.history);
    assert_eq!((form.timeout.text(), form.connect_timeout.text()), ("5", "2 m"));
    assert_eq!(form.draft(), draft, "as they were");
    form.history = false;
    form.timeout.set_text("");
    let written = form.draft().to_text();
    let request = &http_requests::parse(&written).requests[0];
    assert!(request.options.no_log && request.options.no_redirect);
    assert_eq!(request.options.timeout, None, "the default again");
    assert!(written.contains("# @custom x"), "another directive kept");
}
