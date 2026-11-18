//! The UI language: chosen in settings and kept in `settings.toml`, section
//! `[general]` (`language = "ro"`); with no choice, English.

/// The saved language (English if none), applied at startup.
pub fn load() -> String {
    let settings = crate::config::settings();
    let saved = settings
        .is_set(&crate::settings::LANGUAGE)
        .then(|| settings.get(&crate::settings::LANGUAGE));
    // `EDITOR_LANG=en` takes priority (testing, snapshots).
    let forced = std::env::var("EDITOR_LANG")
        .ok()
        .filter(|code| i18n::LANGUAGES.iter().any(|(c, _)| c == code));
    // English by default (never the system's): Romanian and the others are chosen in
    // settings, and a choice saved stays.
    let code = forced.or(saved).unwrap_or_else(|| "en".to_owned());
    i18n::set_locale(&code);
    code
}

/// The saved language applied again (the file rewritten by another program). `true` if it's
/// another one now.
pub fn adopt_saved() -> bool {
    let before = i18n::locale();
    load();
    i18n::locale() != before
}

/// Changes the language and saves it, keeping the rest of the file.
pub fn choose(code: &str) -> bool {
    if !i18n::LANGUAGES.iter().any(|(c, _)| *c == code) || i18n::locale() == code {
        return false;
    }
    i18n::set_locale(code);
    if let Err(e) = crate::config::settings().set(&crate::settings::LANGUAGE, code.to_owned()) {
        tracing::warn!("can't save the language: {e}");
    }
    true
}

/// The languages, for settings: (code, name).
pub fn options() -> Vec<(String, String)> {
    i18n::LANGUAGES.iter().map(|(c, n)| ((*c).to_owned(), (*n).to_owned())).collect()
}
