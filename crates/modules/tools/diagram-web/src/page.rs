//! Our page around draw.io: draw.io in embed mode talks to the window that holds it
//! (`window.parent`), so our page holds it in a frame and passes the messages on — the
//! frame's to the view (`window.ipc.postMessage`), the view's to the frame
//! (`window.__hostReceive`). Written into the install's folder at every opening from the
//! text compiled here, so ours always wins over what's on disk; nothing else of the package
//! is changed.

use std::path::Path;

/// The page the view opens (relative to the install's folder).
pub const HOST_PAGE: &str = "host.html";
/// Its script.
pub const HOST_SCRIPT: &str = "host.js";

/// The page: one frame filling it, made by the script.
pub const HOST_HTML: &str = "<!DOCTYPE html>\n\
<html><head><meta charset=\"utf-8\">\n\
<style>html,body{margin:0;width:100%;height:100%;overflow:hidden;background:transparent}\
iframe{position:fixed;inset:0;border:0;width:100%;height:100%;display:block}</style>\n\
</head><body><script src=\"host.js\"></script></body></html>\n";

/// The script: the frame opened at draw.io's page with this page's own query (what the view
/// chose: the language, the look) after the embed parameters; the frame's messages (only
/// its own) passed to the view as text, the view's to the frame. The page's size followed
/// (a `ResizeObserver`, and the window's own event): the frame told it resized, so draw.io
/// lays itself out again (a web view made small and then placed larger never told it).
pub const HOST_JS: &str = r#""use strict";
(function () {
  var frame = document.createElement("iframe");
  var fixed = "embed=1&proto=json&configure=1&offline=1&stealth=1&lockdown=1&plugins=0" +
    "&pwa=0&splash=0&noExitBtn=1";
  var own = window.location.search.replace(/^\?/, "");
  frame.src = "index.html?" + fixed + (own ? "&" + own : "");
  frame.setAttribute("allow", "clipboard-read; clipboard-write");
  document.body.appendChild(frame);
  // Same origin: the frame's own window is told it resized (draw.io follows its window).
  var resized = function () {
    try {
      if (frame.contentWindow) {
        frame.contentWindow.dispatchEvent(new Event("resize"));
      }
    } catch (e) {}
  };
  var pending = 0;
  var later = function () {
    if (!pending) {
      pending = window.requestAnimationFrame(function () { pending = 0; resized(); });
    }
  };
  if (window.ResizeObserver) {
    new ResizeObserver(later).observe(document.documentElement);
  }
  window.addEventListener("resize", later);
  frame.addEventListener("load", later);
  // Printing can't work here (no print dialog, no new window): its key does nothing, as
  // its menu entry is hidden.
  frame.addEventListener("load", function () {
    try {
      var inner = frame.contentWindow;
      inner.print = function () {};
      inner.addEventListener("keydown", function (event) {
        var key = (event.key || "").toLowerCase();
        if ((event.metaKey || event.ctrlKey) && key === "p") {
          event.preventDefault();
          event.stopImmediatePropagation();
        }
      }, true);
    } catch (e) {}
  });
  window.addEventListener("message", function (event) {
    if (event.source !== frame.contentWindow || !window.ipc) {
      return;
    }
    var data = event.data;
    if (typeof data !== "string") {
      try { data = JSON.stringify(data); } catch (e) { return; }
    }
    window.ipc.postMessage(data);
    // Its interface made (or a diagram loaded): laid out for the size it has now, which
    // may have changed while it was being made.
    if (data.indexOf('"event":"init"') >= 0 || data.indexOf('"event":"load"') >= 0) {
      later();
    }
  });
  window.__hostReceive = function (text) {
    if (frame.contentWindow) {
      frame.contentWindow.postMessage(text, "*");
    }
  };
})();
"#;

/// The page's address (relative to the folder): its query says the interface's language
/// (`ro`, else `en`) and draw.io's look (`ui`, `dark`).
pub fn entry(locale: &str, look: (&str, &str)) -> String {
    let lang = if locale.to_ascii_lowercase().starts_with("ro") { "ro" } else { "en" };
    format!("{HOST_PAGE}?lang={lang}&ui={}&dark={}", look.0, look.1)
}

/// Our page and script written into the install's folder `dir` (on a worker).
pub fn write(dir: &Path) -> std::io::Result<()> {
    files::write_atomic(&dir.join(HOST_PAGE), HOST_HTML.as_bytes())?;
    files::write_atomic(&dir.join(HOST_SCRIPT), HOST_JS.as_bytes())
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
