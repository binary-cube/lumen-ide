//! A line break at the end of every saved file (on by default, `[editor] final_newline` in
//! `settings.toml`). A project's `.editorconfig`
//! (`insert_final_newline`) decides for its files.

pub fn enabled() -> bool {
    crate::config::settings().get(&crate::settings::FINAL_NEWLINE)
}
