//! OpenAPI and Swagger descriptions as requests: their server, paths with variables, the
//! parameters, a body from its example or its schema, the security's header.

use super::*;

const OPENAPI_YAML: &str = r#"
openapi: 3.0.3
info:
  title: Pet Store
servers:
  - url: https://pets.example.com/v1
security:
  - bearer: []
components:
  securitySchemes:
    bearer:
      type: http
      scheme: bearer
  schemas:
    Pet:
      type: object
      properties:
        id: { type: integer }
        name: { type: string, example: Rex }
        owner: { $ref: '#/components/schemas/Owner' }
    Owner:
      type: object
      properties:
        email: { type: string, format: email }
        pet: { $ref: '#/components/schemas/Pet' }
paths:
  /pets/{petId}:
    parameters:
      - name: petId
        in: path
        required: true
    get:
      tags: [pets]
      summary: Get a pet
      operationId: getPet
      parameters:
        - name: fields
          in: query
        - name: X-Trace
          in: header
  /pets:
    post:
      tags: [pets]
      summary: Add a pet
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
"#;

#[test]
fn an_openapi_description_in_yaml_becomes_requests() {
    let imported = crate::import::from_text(OPENAPI_YAML).unwrap();
    assert_eq!((imported.name.as_str(), imported.count), ("Pet Store", 2));
    let parsed = crate::parse(&imported.requests);
    let by = |method: &str| parsed.requests.iter().find(|r| r.method == method).unwrap();
    let get = by("GET");
    assert_eq!(get.name.as_deref(), Some("getPet"));
    assert_eq!(get.method, "GET");
    assert_eq!(get.url, "{{baseUrl}}/pets/{{petId}}?fields={{fields}}");
    assert!(get.headers.iter().any(|h| h.name == "X-Trace" && h.value == "{{X-Trace}}"));
    assert!(get.headers.iter().any(|h| h.name == "Authorization" && h.value == "Bearer {{token}}"));
    assert!(imported.requests.contains("### pets / Get a pet\n"));
    let post = by("POST");
    let crate::BodyPart::Text(body) = &post.body[0] else { panic!("a body") };
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["name"], "Rex", "its example");
    assert_eq!(body["owner"]["email"], "user@example.com", "a sample by its format");
    assert!(body["owner"]["pet"].is_null(), "a loop of references cut");
    let env: serde_json::Value =
        serde_json::from_str(imported.environments.as_deref().unwrap()).unwrap();
    assert_eq!(env["$shared"]["baseUrl"], "https://pets.example.com/v1");
    assert_eq!(env["$shared"]["token"], "");
}

#[test]
fn a_swagger_description_becomes_requests() {
    let json = r#"{
      "swagger": "2.0", "info": {"title": "Old"}, "host": "api.e.com", "basePath": "/v2", "schemes": ["https"],
      "securityDefinitions": {"key": {"type": "apiKey", "in": "header", "name": "X-API-Key"}},
      "paths": {"/users": {"post": {"operationId": "addUser", "parameters": [
        {"in": "body", "name": "body", "schema": {"type": "object", "properties": {"name": {"type": "string"}}}}
      ]}}}
    }"#;
    let imported = crate::import::from_json(json).unwrap();
    let request = &crate::parse(&imported.requests).requests[0];
    assert_eq!(request.url, "{{baseUrl}}/users");
    assert!(request.headers.iter().any(|h| h.name == "X-API-Key" && h.value == "{{apiKey}}"));
    assert!(
        request.headers.iter().any(|h| h.name == "Content-Type" && h.value == "application/json")
    );
    let env: serde_json::Value =
        serde_json::from_str(imported.environments.as_deref().unwrap()).unwrap();
    assert_eq!(env["$shared"]["baseUrl"], "https://api.e.com/v2");
}

#[test]
fn what_a_description_names_never_becomes_a_variable_of_its_own() {
    let spec = serde_json::json!({"openapi": "3.0.0", "paths": {
        "/users/{id}/{{$auth.token(\"x\")}}": {"post": {
            "parameters": [
                {"name": "q}}&{{$env.HOME", "in": "query"},
                {"name": "X-{{$auth.token(\"x\")}}", "in": "header"}
            ],
            "requestBody": {"content": {"application/json": {"example": {"a": "{{$auth.token(\"x\")}}"}}}}
        }}
    }});
    let (_, operations, _) = read(&spec);
    let op = &operations[0];
    let all = format!("{} {:?} {:?}", op.url, op.headers, op.body);
    assert!(!all.contains("{{$"), "{all}");
    assert!(op.url.starts_with("{{baseUrl}}/users/{{id}}/"), "{}", op.url);
}

#[test]
fn a_sample_and_a_yaml_are_bounded() {
    // A wide schema: its sample stops at the budget.
    let properties: serde_json::Map<String, Value> =
        (0..5000).map(|i| (format!("p{i}"), serde_json::json!({"type": "string"}))).collect();
    let schema = serde_json::json!({"type": "object", "properties": properties});
    let made = sample(&Value::Null, Some(&schema), 0, &mut Sampling::default());
    let strings = made.as_object().unwrap().values().filter(|v| v.is_string()).count();
    assert!(strings < SAMPLE_NODES, "{strings}");
    // Aliases of aliases: refused before it's built.
    let mut yaml = String::from("a: &a [x, x, x, x, x, x, x, x, x, x]\n");
    for level in 1..9 {
        let previous = (b'a' + level - 1) as char;
        let name = (b'a' + level) as char;
        let list = vec![format!("*{previous}"); 10].join(", ");
        yaml.push_str(&format!("{name}: &{name} [{list}]\n"));
    }
    assert!(yaml_to_json(&yaml).is_err());
    assert!(yaml_to_json("a: &x [1, 2]\nb: *x\n").is_ok(), "a small one is read");
}
