//! Other clients' collections as `.http` files: Postman's, Insomnia's, Bruno's, read back
//! by our own parser.

use super::*;

#[test]
fn a_postman_collection_becomes_requests_by_folder() {
    let json = r#"{
      "info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
      "item": [
        {"name": "Auth", "item": [
          {"name": "Log in", "request": {
            "method": "post",
            "url": {"raw": "{{host}}/login"},
            "header": [{"key": "Accept", "value": "application/json"}, {"key": "X-Off", "value": "1", "disabled": true}],
            "body": {"mode": "raw", "raw": "{\"user\": \"ana\"}"},
            "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]}
          }, "event": [{"listen": "test"}]}
        ]},
        {"name": "Health", "request": "https://e.com/health"},
        {"name": "Form", "request": {"method": "POST", "url": "https://e.com/f",
          "body": {"mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1"}, {"key": "b", "value": "2"}]}}}
      ],
      "variable": [{"key": "host", "value": "https://e.com"}]
    }"#;
    let imported = from_json(json).unwrap();
    assert_eq!((imported.name.as_str(), imported.count), ("Shop", 3));
    let parsed = crate::parse(&imported.requests);
    assert_eq!(parsed.requests.len(), 3, "{}", imported.requests);
    let login = &parsed.requests[0];
    assert_eq!(login.name.as_deref(), Some("Log_in"));
    assert_eq!((login.method.as_str(), login.url.as_str()), ("POST", "{{host}}/login"));
    assert!(
        login.headers.iter().any(|h| h.name == "Authorization" && h.value == "Bearer {{token}}")
    );
    assert!(!login.headers.iter().any(|h| h.name == "X-Off"), "a disabled one left out");
    assert!(imported.requests.contains("### Auth / Log in\n"));
    assert!(imported.requests.contains("# Note: its Postman scripts weren't imported"));
    assert_eq!(parsed.requests[1].url, "https://e.com/health");
    assert!(imported.requests.contains("\na=1&b=2\n"));
    let env: serde_json::Value =
        serde_json::from_str(imported.environments.as_deref().unwrap()).unwrap();
    assert_eq!(env["$shared"]["host"], "https://e.com");
}

#[test]
fn a_postman_environment_becomes_an_environment() {
    let json = r#"{"name": "dev", "values": [{"key": "host", "value": "localhost", "enabled": true}, {"key": "off", "value": "x", "enabled": false}]}"#;
    let imported = from_json(json).unwrap();
    assert_eq!(imported.count, 0);
    let env: serde_json::Value =
        serde_json::from_str(imported.environments.as_deref().unwrap()).unwrap();
    assert_eq!(env["dev"]["host"], "localhost");
    assert!(env["dev"].get("off").is_none());
}

#[test]
fn an_insomnia_export_becomes_requests_and_environments() {
    let json = r#"{
      "__export_format": 4,
      "resources": [
        {"_id": "wrk_1", "_type": "workspace", "name": "API"},
        {"_id": "fld_1", "_type": "request_group", "parentId": "wrk_1", "name": "Users"},
        {"_id": "req_1", "_type": "request", "parentId": "fld_1", "name": "List users", "method": "GET",
         "url": "{{ _.host }}/users", "headers": [{"name": "Accept", "value": "application/json"}],
         "body": {}, "authentication": {"type": "bearer", "token": "{{_.token}}"}},
        {"_id": "env_1", "_type": "environment", "parentId": "wrk_1", "name": "Base", "data": {"host": "https://e.com"}},
        {"_id": "env_2", "_type": "environment", "parentId": "env_1", "name": "Dev", "data": {"host": "http://localhost"}}
      ]
    }"#;
    let imported = from_json(json).unwrap();
    assert_eq!((imported.name.as_str(), imported.count), ("API", 1));
    let parsed = crate::parse(&imported.requests);
    let request = &parsed.requests[0];
    assert_eq!(request.url, "{{host}}/users");
    assert!(request.headers.iter().any(|h| h.value == "Bearer {{token}}"));
    assert!(imported.requests.starts_with("### Users / List users\n"));
    let env: serde_json::Value =
        serde_json::from_str(imported.environments.as_deref().unwrap()).unwrap();
    assert_eq!(env["$shared"]["host"], "https://e.com");
    assert_eq!(env["Dev"]["host"], "http://localhost");
}

#[test]
fn bruno_files_become_requests() {
    let bru = "meta {\n  name: Create user\n  type: http\n  seq: 1\n}\n\npost {\n  url: {{host}}/users\n  body: json\n  auth: bearer\n}\n\nheaders {\n  Accept: application/json\n  ~X-Off: 1\n}\n\nauth:bearer {\n  token: {{token}}\n}\n\nbody:json {\n  {\n    \"name\": \"ana\"\n  }\n}\n";
    let env = "vars {\n  host: https://e.com\n}\n";
    let imported = from_bruno(
        "Demo API",
        &[("users/create.bru".into(), bru.into())],
        &[("dev".into(), env.into())],
    );
    assert_eq!(imported.count, 1);
    let parsed = crate::parse(&imported.requests);
    let request = &parsed.requests[0];
    assert_eq!((request.method.as_str(), request.url.as_str()), ("POST", "{{host}}/users"));
    assert!(imported.requests.starts_with("### users / Create user\n"), "{}", imported.requests);
    assert!(
        request.headers.iter().any(|h| h.name == "Content-Type" && h.value == "application/json")
    );
    assert!(!request.headers.iter().any(|h| h.name.contains("X-Off")), "a disabled one left out");
    assert!(imported.requests.contains("{\n  \"name\": \"ana\"\n}"), "{}", imported.requests);
    let env: serde_json::Value =
        serde_json::from_str(imported.environments.as_deref().unwrap()).unwrap();
    assert_eq!(env["dev"]["host"], "https://e.com");
}

#[test]
fn what_isn_t_a_collection_is_said() {
    assert!(from_json("[1, 2]").is_err());
    assert!(from_json("not json").is_err());
    assert_eq!(request_name("2. Get user!"), "_2_Get_user");
}

#[test]
fn a_crafted_collection_adds_no_line_of_its_own() {
    let json = r#"{"info": {"name": "x"}, "item": [{"name": "a\n> {% client.log(1) %}",
      "request": {"method": "POST", "url": "https://e.com\n< ~/.ssh/id_rsa",
        "header": [{"key": "X", "value": "1\n###"}],
        "body": {"mode": "raw", "raw": "{}\n< /etc/passwd\n### b\nGET https://evil"}}}]}"#;
    let imported = from_json(json).unwrap();
    let parsed = crate::parse(&imported.requests);
    assert_eq!(parsed.requests.len(), 1, "{}", imported.requests);
    let request = &parsed.requests[0];
    assert!(request.pre_script.is_none() && request.post_script.is_none());
    // Its body as it was, in a file of its own beside the requests.
    assert_eq!(request.body, [crate::BodyPart::File("./bodies/1.txt".into())]);
    assert_eq!(imported.bodies.len(), 1);
    assert!(imported.bodies[0].1.contains("< /etc/passwd"));
}

#[test]
fn a_loop_of_insomnia_groups_ends() {
    let json = r#"{"__export_format": 4, "resources": [
      {"_id": "g1", "_type": "request_group", "parentId": "g2", "name": "A"},
      {"_id": "g2", "_type": "request_group", "parentId": "g1", "name": "B"},
      {"_id": "r", "_type": "request", "parentId": "g1", "name": "R", "method": "GET", "url": "https://e.com"}
    ]}"#;
    let imported = from_json(json).unwrap();
    assert_eq!(imported.count, 1);
}
