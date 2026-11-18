use super::*;

#[test]
fn the_entry_says_the_language_and_the_look() {
    assert_eq!(entry("ro", ("kennedy", "1")), "host.html?lang=ro&ui=kennedy&dark=1");
    assert_eq!(entry("en-US", ("kennedy", "0")), "host.html?lang=en&ui=kennedy&dark=0");
    assert_eq!(entry("de", ("kennedy", "0")), "host.html?lang=en&ui=kennedy&dark=0");
}

#[test]
fn the_host_page_frames_draw_io_in_embed_mode_and_passes_messages() {
    assert!(HOST_HTML.contains("<script src=\"host.js\"></script>"));
    for param in [
        "embed=1",
        "proto=json",
        "configure=1",
        "offline=1",
        "stealth=1",
        "lockdown=1",
        "plugins=0",
        "pwa=0",
        "splash=0",
        "noExitBtn=1",
    ] {
        assert!(HOST_JS.contains(param), "{param}");
    }
    assert!(HOST_JS.contains("\"index.html?\""));
    // Only the frame's messages, to the view; the view's to the frame.
    assert!(HOST_JS.contains("event.source !== frame.contentWindow"));
    assert!(HOST_JS.contains("window.ipc.postMessage(data)"));
    assert!(HOST_JS.contains("window.__hostReceive = function"));
    // The frame fills the page and is told when the page's size changes.
    assert!(HOST_HTML.contains("position:fixed;inset:0"));
    assert!(HOST_JS.contains("ResizeObserver"));
    assert!(HOST_JS.contains("dispatchEvent(new Event(\"resize\"))"));
}

#[test]
fn ours_is_written_over_what_is_there() {
    let dir = std::env::temp_dir().join(format!("diagram-web-page-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(HOST_PAGE), "changed").unwrap();
    write(&dir).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(HOST_PAGE)).unwrap(), HOST_HTML);
    assert_eq!(std::fs::read_to_string(dir.join(HOST_SCRIPT)).unwrap(), HOST_JS);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Printing can't work in the app's page: the frame's print does nothing and ⌘P / Ctrl+P is
/// stopped before draw.io sees it (its menu entry is hidden too).
#[test]
fn printing_is_stopped_in_the_frame() {
    assert!(HOST_JS.contains("inner.print = function () {}"));
    assert!(HOST_JS.contains("(event.metaKey || event.ctrlKey) && key === \"p\""));
    assert!(HOST_JS.contains("stopImmediatePropagation"));
}
