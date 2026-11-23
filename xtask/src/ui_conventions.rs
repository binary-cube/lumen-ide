//! The UI guide's rules `check-deps` reads in the sources (plan 95c, `docs/ui/`), so its
//! recipe isn't lost:
//!
//! - every component of `ui-components` (a `pub mod` of its `lib.rs`) has its page in
//!   `docs/ui/components/` (`data_grid` → `data-grid.md`), but the helpers and contracts
//!   listed in [`NO_PAGE`] with why;
//! - no color written into drawing code: a fixed color (`Rgba::rgb(0x…)`) in a module or in
//!   the UI crates is a named `const` (or `static`), its comment saying why it's fixed —
//!   the default themes (`palette.rs`) are the colors themselves;
//! - every component that's a form's part (`impl … Control for`) has tests that press it
//!   and hover it (its own test file, or its folder's);
//! - no module draws a control of its own (`fn draw_button`, `fn paint_scrollbar`…): the
//!   shared one, an option added to it if it lacks something.
//!
//! Test code is left out.

use std::path::Path;

use crate::app_sources::{Source, code_of, read_sources};

/// The modules of `ui-components` without a page of their own, and why.
pub const NO_PAGE: &[(&str, &str)] = &[
    ("ds", "the design system's tokens and controls: tokens.md, controls.md, button.md…"),
    ("painter", "what a paint is handed (Pen, LayerPainter): an argument, not a component"),
    ("scroll_state", "a list's scroll position, used by ScrollThumb: scrollbar.md"),
    ("decorations", "the contract a theme's decorations module fills (theming)"),
    ("services", "the Services window's contribution point (a contract)"),
    ("shortcuts", "the commands' keys as the user has them (a lookup)"),
    ("snippets", "code highlighted outside the editor (a port the app fills)"),
    ("code_tokens", "a small tokenizer coloring code rows (a helper of the canvas and diff)"),
];

/// Where a fixed color may be written as it is: the default themes.
const THEMES: &str = "crates/ui/ui-components/src/palette.rs";

/// The names of a control drawn by hand in a module.
const HAND_DRAWN: &[&str] =
    &["button", "checkbox", "switch", "scrollbar", "radio", "toggle", "spinner", "keycap", "badge"];

/// The violations of the rules above under `root`.
pub fn violations(root: &Path) -> Vec<String> {
    let components = root.join("crates/ui/ui-components");
    let mut found = missing_pages(
        &std::fs::read_to_string(components.join("src/lib.rs")).unwrap_or_default(),
        &|page| root.join("docs/ui/components").join(page).exists(),
    );
    for (dir, prefix) in [
        ("crates/modules", "crates/modules"),
        ("crates/ui/ui-components/src", "crates/ui/ui-components/src"),
        ("crates/ui/ui-workbench/src", "crates/ui/ui-workbench/src"),
    ] {
        for source in read_sources(&root.join(dir)) {
            if is_test(&source.path) {
                continue;
            }
            let path = format!("{prefix}/{}", source.path);
            if path != THEMES {
                found.extend(loose_colors(&path, &source.text));
            }
            if dir == "crates/modules" {
                found.extend(hand_drawn(&path, &source.text));
            }
        }
    }
    found.extend(untested_controls(&components));
    found
}

fn is_test(path: &str) -> bool {
    path.split('/').any(|part| part == "tests")
}

/// The `pub mod`s of `lib` without a page (`exists` asked of each page's file name).
pub fn missing_pages(lib: &str, exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    lib.lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .filter_map(|rest| rest.strip_suffix(';'))
        .filter(|module| !NO_PAGE.iter().any(|(name, _)| name == module))
        .filter(|module| !exists(&format!("{}.md", module.replace('_', "-"))))
        .map(|module| {
            format!(
                "ui-components: `{module}` has no page in docs/ui/components/ (write one, or \
                 say why not in ui_conventions::NO_PAGE)"
            )
        })
        .collect()
}

/// The fixed colors of `text` written outside a `const` or `static`.
pub fn loose_colors(path: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut item = "";
    for (i, line) in text.lines().enumerate() {
        if line == "#[cfg(test)]" {
            break;
        }
        // The item a line is in: the last one starting at the line's start.
        if line.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            item = line;
        }
        if !code_of(line).contains("Rgba::rgb(0x") {
            continue;
        }
        let constant = ["const ", "static ", "pub const ", "pub(crate) const ", "pub static "]
            .iter()
            .any(|start| item.starts_with(start));
        if !constant {
            found.push(format!(
                "{path}:{}: a color written into code: a named `const` saying why it's fixed (or \
                 the theme's)",
                i + 1
            ));
        }
    }
    found
}

/// A module's own drawing of a control (`fn draw_button`, `fn paint_scrollbar`…).
pub fn hand_drawn(path: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let code = code_of(line);
        for verb in ["fn draw_", "fn paint_"] {
            let Some(at) = code.find(verb) else { continue };
            let name = &code[at + verb.len()..];
            let name = name.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
            if HAND_DRAWN.contains(&name) {
                found.push(format!(
                    "{path}:{}: a module draws its own {name}: the shared one (ui-components), an \
                     option added to it",
                    i + 1
                ));
            }
        }
    }
    found
}

/// The components that are a form's part without tests pressing and hovering them.
fn untested_controls(components: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for source in read_sources(&components.join("src")) {
        if !source.text.lines().any(|l| l.starts_with("impl") && l.contains(" Control for ")) {
            continue;
        }
        let tests = tests_of(components, &source);
        if !(tests.contains("press") || tests.contains("click")) || !tests.contains("hover") {
            found.push(format!(
                "crates/ui/ui-components/src/{}: a control without tests pressing and hovering it",
                source.path
            ));
        }
    }
    found
}

/// The tests of `source` (its own file, else its folder's `mod.rs`'s), as text.
fn tests_of(components: &Path, source: &Source) -> String {
    let src = components.join("src");
    let mut file = src.join(&source.path);
    loop {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if let Some(path) = test_path(&text) {
            let dir = file.parent().unwrap_or(&src);
            return std::fs::read_to_string(dir.join(path)).unwrap_or_default();
        }
        // A folder's file: its `mod.rs` (or the folder's own `.rs`) has the tests.
        let Some(dir) = file.parent().filter(|d| d.starts_with(&src) && *d != src) else {
            return String::new();
        };
        let module = dir.join("mod.rs");
        file = if module.exists() && file != module { module } else { dir.with_extension("rs") };
        if !file.exists() {
            return String::new();
        }
    }
}

/// The file a `#[path = "…"] mod tests;` names.
fn test_path(text: &str) -> Option<&str> {
    let at = text.find("#[path = \"")?;
    let rest = &text[at + "#[path = \"".len()..];
    let end = rest.find('"')?;
    rest[end..].contains("mod tests").then(|| &rest[..end])
}

#[cfg(test)]
#[path = "../tests/unit/ui_conventions.rs"]
mod tests;
