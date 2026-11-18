//! JSONPath as chained requests read a response.

use super::*;

fn doc() -> Value {
    serde_json::json!({
        "data": {
            "items": [
                {"id": 7, "name": "ana", "tags": ["a"]},
                {"id": 9, "name": "bob", "active": true}
            ],
            "total": 2
        },
        "my key": "spaced"
    })
}

fn texts(path: &str) -> Vec<String> {
    select(&doc(), path).unwrap().into_iter().map(as_text).collect()
}

#[test]
fn members_elements_and_wildcards() {
    assert_eq!(texts("$.data.items[0].id"), ["7"]);
    assert_eq!(texts("$.data.items[-1].name"), ["bob"]);
    assert_eq!(texts("$['my key']"), ["spaced"]);
    assert_eq!(texts("$.data.items[*].id"), ["7", "9"]);
    assert_eq!(texts("data.total"), ["2"], "without the `$.`");
    assert_eq!(texts("$..name"), ["ana", "bob"]);
    assert_eq!(texts("$.data.items[5]"), Vec::<String>::new());
    assert_eq!(texts("$"), [doc().to_string()]);
}

#[test]
fn filters() {
    assert_eq!(texts("$.data.items[?(@.id > 8)].name"), ["bob"]);
    assert_eq!(texts("$.data.items[?(@.name == 'ana')].id"), ["7"]);
    assert_eq!(texts("$.data.items[?(@.active)].id"), ["9"]);
    assert_eq!(texts("$.data.items[?(@.active == true)].id"), ["9"]);
    assert_eq!(texts("$.data.items[?(@.id != 7)].id"), ["9"]);
    assert!(select(&doc(), "$.data[").is_err());
}

#[test]
fn the_keys_at_a_path_for_completion() {
    let keys = children(&doc(), "$.data");
    assert_eq!(keys.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), [".items", ".total"]);
    assert_eq!(keys[0].1, "[…] 2 items");
    let items = children(&doc(), "$.data.items");
    assert_eq!(items[1].0, "[1]");
    assert_eq!(
        children(&doc(), "$").iter().find(|(k, _)| k.contains("my")).map(|(k, _)| k.as_str()),
        Some("['my key']")
    );
}
