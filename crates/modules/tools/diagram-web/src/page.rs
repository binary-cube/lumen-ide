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
///
/// Where the user is (`crate::state`), through draw.io's editor (same origin): caught as it
/// makes its first `load` answer (`createLoadMessage`, wrapped once draw.io said `init`);
/// then its page, zoom, position, selection and whether a cell is being typed into or a
/// dialog is open are told (`hostState`) a moment after they change; `hostRestore` (kept
/// until the diagram has loaded) puts them back and says whether an undo or redo past the
/// page's own history is offered — then draw.io's undo and redo, once its own history has
/// nothing left, ask the view (`hostUndo`, `hostRedo`). The user acting on it (a press, a
/// key, the wheel) is told at most every 2 s (`hostTouched`). None of it reaches the frame.
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
  var post = function (message) {
    if (window.ipc) {
      window.ipc.postMessage(JSON.stringify(message));
    }
  };
  // draw.io's editor (caught at its first load answer), what to put back once the diagram
  // has loaded, and whether an undo or a redo past its own history is offered.
  var ui = null;
  var restore = null;
  var more = { past: false, future: false };
  var told = 0;
  var tell = function () {
    if (told) {
      return;
    }
    told = window.setTimeout(function () {
      told = 0;
      try {
        var graph = ui.editor.graph;
        var view = graph.view;
        var cells = graph.getSelectionCells().slice(0, 1000);
        post({
          event: "hostState",
          page: Math.max(0, ui.getSelectedPageIndex() || 0),
          scale: view.scale,
          tx: view.translate.x,
          ty: view.translate.y,
          left: ui.diagramContainer.scrollLeft,
          top: ui.diagramContainer.scrollTop,
          selection: cells.map(function (cell) { return String(cell.id); }),
          busy: graph.isEditing() || (ui.dialogs != null && ui.dialogs.length > 0)
        });
      } catch (e) {}
    }, 300);
  };
  var follow = function (caught) {
    ui = caught;
    var inner = frame.contentWindow;
    var graph = ui.editor.graph;
    var events = inner.mxEvent;
    [events.SCALE, events.TRANSLATE, events.SCALE_AND_TRANSLATE].forEach(function (name) {
      graph.view.addListener(name, tell);
    });
    graph.getSelectionModel().addListener(events.CHANGE, tell);
    graph.addListener(events.EDITING_STARTED, tell);
    graph.addListener(events.EDITING_STOPPED, tell);
    ui.addListener("pageSelected", tell);
    ui.diagramContainer.addEventListener("scroll", tell, { passive: true });
    // The user acting on it (a press, a key, the wheel): told at most every 2 s — the view
    // isn't put to sleep while it's used.
    var touched_at = 0;
    var touched = function () {
      var now = Date.now();
      if (now - touched_at >= 2000) {
        touched_at = now;
        post({ event: "hostTouched" });
      }
    };
    ["pointerdown", "keydown", "wheel"].forEach(function (name) {
      inner.addEventListener(name, touched, { capture: true, passive: true });
    });
    ["showDialog", "hideDialog"].forEach(function (name) {
      var own = ui[name];
      ui[name] = function () {
        var result = own.apply(ui, arguments);
        tell();
        return result;
      };
    });
    // Undo and redo go on past the page's own history: the view's, once it has nothing left.
    var past = function (name, flag, event) {
      var can = ui["can" + name];
      var act = ui[name.toLowerCase()];
      ui["can" + name] = function () { return can.apply(ui, arguments) || more[flag]; };
      ui[name.toLowerCase()] = function () {
        if (!can.apply(ui, arguments) && more[flag]) {
          more[flag] = false;
          post({ event: event });
          return;
        }
        return act.apply(ui, arguments);
      };
    };
    past("Undo", "past", "hostUndo");
    past("Redo", "future", "hostRedo");
    ui.updateActionStates();
  };
  var put_back = function () {
    var state = restore;
    restore = null;
    if (!ui || !state) {
      return;
    }
    try {
      var graph = ui.editor.graph;
      if (ui.pages && state.page < ui.pages.length && ui.currentPage !== ui.pages[state.page]) {
        // Quietly: putting it back isn't a change to undo.
        ui.selectPage(ui.pages[state.page], true);
      }
      graph.view.scaleAndTranslate(state.scale, state.tx, state.ty);
      ui.diagramContainer.scrollLeft = state.left;
      ui.diagramContainer.scrollTop = state.top;
      var cells = (state.selection || []).map(function (id) { return graph.model.getCell(id); });
      graph.setSelectionCells(cells.filter(function (cell) { return cell != null; }));
      ui.updateActionStates();
    } catch (e) {}
  };
  var catch_editor = function () {
    try {
      var editors = frame.contentWindow.EditorUi;
      if (ui || !editors || editors.prototype.__hostCaught) {
        return;
      }
      editors.prototype.__hostCaught = true;
      var own = editors.prototype.createLoadMessage;
      editors.prototype.createLoadMessage = function () {
        if (!ui) {
          follow(this);
        }
        return own.apply(this, arguments);
      };
    } catch (e) {}
  };
  window.addEventListener("message", function (event) {
    if (event.source !== frame.contentWindow || !window.ipc) {
      return;
    }
    var data = event.data;
    if (typeof data !== "string") {
      try { data = JSON.stringify(data); } catch (e) { return; }
    }
    window.ipc.postMessage(data);
    // A draw.io starting (its page made anew): its editor caught again, once it's made.
    if (data.indexOf('"event":"configure"') >= 0) {
      ui = null;
    }
    if (data.indexOf('"event":"init"') >= 0) {
      catch_editor();
    }
    // Its interface made (or a diagram loaded): laid out for the size it has now, which
    // may have changed while it was being made; loaded, where the user was put back.
    if (data.indexOf('"event":"init"') >= 0 || data.indexOf('"event":"load"') >= 0) {
      later();
    }
    // A timer, not a frame: a page not shown yet draws no frames.
    if (data.indexOf('"event":"load"') >= 0) {
      window.setTimeout(put_back, 0);
    }
  });
  window.__hostReceive = function (text) {
    // Ours: kept here, never the frame's.
    if (text.indexOf('"action":"hostRestore"') >= 0) {
      try {
        var message = JSON.parse(text);
        restore = message.state || null;
        more.past = message.past === true;
        more.future = message.future === true;
        if (ui) {
          ui.updateActionStates();
        }
      } catch (e) {}
      return;
    }
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
