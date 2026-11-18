//! The HTTP client's button: no window of its own, it opens the HTTP tab.

use super::*;

#[test]
fn its_button_opens_the_http_tab() {
    let tool = HttpTool;
    assert_eq!(
        tool.content(),
        ToolContent::Command { command: TOGGLE.0, tab: Some(crate::collection_view::EXTENSION) }
    );
    assert!(tool.create().is_none(), "no panel made");
}
