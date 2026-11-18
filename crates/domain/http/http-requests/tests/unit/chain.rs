//! Requests chained by their responses: the references, the dependencies, the order.

use super::*;

#[test]
fn a_response_s_variables_are_read() {
    let body = ResponseRef::parse("login.response.body.$.token").unwrap();
    assert_eq!((body.request.as_str(), body.part), ("login", ResponsePart::Body("$.token".into())));
    assert_eq!(ResponseRef::parse("a.response.body").unwrap().part, ResponsePart::Body("*".into()));
    assert_eq!(
        ResponseRef::parse("a.response.headers.Location").unwrap().part,
        ResponsePart::Header("Location".into())
    );
    assert_eq!(ResponseRef::parse("a.response.status").unwrap().part, ResponsePart::Status);
    assert_eq!(ResponseRef::parse("host"), None);
    assert_eq!(ResponseRef::parse("a.response.cookies"), None);
}

const FILE: &str = "\
# @name login
POST https://e.com/login

###
# @name me
GET https://e.com/me
Authorization: Bearer {{login.response.body.$.token}}

###
GET https://e.com/orders?user={{me.response.body.$.id}}&s={{login.response.headers.X-Session}}
";

#[test]
fn a_request_runs_after_the_ones_it_uses() {
    let file = crate::parse(FILE);
    assert_eq!(dependencies(&file.requests[2]), ["me", "login"]);
    let none = |_: &str| false;
    assert_eq!(order(&file, 2, &none), Ok(vec![0, 1, 2]));
    // `login`'s response kept: only `me` and itself.
    let kept = |name: &str| name == "login";
    assert_eq!(order(&file, 2, &kept), Ok(vec![1, 2]));
    assert_eq!(
        edges(&file),
        [
            (0, 1, "login.response.body.$.token".to_owned()),
            (1, 2, "me.response.body.$.id".to_owned()),
            (0, 2, "login.response.headers.X-Session".to_owned()),
        ]
    );
}

#[test]
fn a_loop_and_an_unknown_request_are_said() {
    let looped = crate::parse(
        "# @name a\nGET https://e.com/{{b.response.status}}\n\n###\n# @name b\nGET https://e.com/{{a.response.status}}\n",
    );
    assert_eq!(
        order(&looped, 0, &|_| false),
        Err(ChainError::Loop(vec!["a".into(), "b".into(), "a".into()]))
    );
    let unknown = crate::parse("GET https://e.com/{{nope.response.status}}\n");
    assert_eq!(order(&unknown, 0, &|_| false), Err(ChainError::Unknown("nope".into())));
}

#[test]
fn the_whole_file_in_an_order_its_chain_allows() {
    let file = crate::parse(
        "GET https://e.com/{{b.response.status}}\n\n###\n# @name b\nGET https://e.com/b\n\n###\nGET https://e.com/c\n",
    );
    assert_eq!(order_all(&file), Ok(vec![1, 0, 2]), "b before the first, each once");
}
