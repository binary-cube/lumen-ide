//! XPath over a response's XML: children, descendants, positions, attributes, text.

use super::*;

const XML: &str = r#"<?xml version="1.0"?>
<login status="ok">
  <token>t0k</token>
  <users>
    <user id="1"><name>Ana</name></user>
    <user id="2"><name>Ion</name></user>
  </users>
</login>"#;

#[test]
fn paths_select_what_they_say() {
    assert_eq!(select(XML, "/login/token").unwrap(), ["t0k"]);
    assert_eq!(select(XML, "/login/@status").unwrap(), ["ok"]);
    assert_eq!(select(XML, "//name").unwrap(), ["Ana", "Ion"]);
    assert_eq!(select(XML, "//user[2]/name").unwrap(), ["Ion"]);
    assert_eq!(select(XML, "//user[@id='1']/name/text()").unwrap(), ["Ana"]);
    assert_eq!(select(XML, "//user[last()]/@id").unwrap(), ["2"]);
    assert_eq!(select(XML, "/login/*").unwrap().len(), 2);
    assert!(select(XML, "//user[x>1]").is_err());
    assert!(select("not xml", "/a").is_err());
}

#[test]
fn descendants_of_nested_nodes_are_each_found_once() {
    let xml = "<a><b><b><c>1</c></b></b><c>2</c></a>";
    assert_eq!(select(xml, "//b//c").unwrap(), ["1"]);
    assert_eq!(select(xml, "//c").unwrap(), ["1", "2"]);
}
