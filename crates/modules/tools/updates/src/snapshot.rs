//! A version found, made up (no network): what the app's `--snapshot` shows What's New
//! with, and the tests' version.

use std::sync::Arc;

use self_update::check::Available;

use crate::state::{self, Phase};

/// `version` found, with `notes` (the release's text, the app's part between its marks);
/// `None` if `version` isn't one.
pub(crate) fn made_up(version: &str, notes: &str) -> Option<Arc<Available>> {
    let parsed = app_updates::Version::parse(version)?;
    let file = app_updates::ManifestFile {
        platform: "windows-x64".into(),
        name: format!("app-{version}-windows-x64.zip"),
        size: 70 * 1024 * 1024,
        sha256: "0".repeat(64),
    };
    Some(Arc::new(Available {
        release: app_updates::Release {
            version: parsed.clone(),
            tag: format!("v{version}"),
            name: format!("App {version}"),
            notes: notes.to_owned(),
            prerelease: false,
            published: "2026-12-01T10:00:00Z".into(),
            page: format!("https://github.com/o/r/releases/tag/v{version}"),
            assets: Vec::new(),
        },
        manifest: app_updates::Manifest {
            version: parsed,
            date: "2026-12-01".into(),
            files: vec![file.clone()],
        },
        file,
        url: format!(
            "https://github.com/o/r/releases/download/v{version}/app-{version}-windows-x64.zip"
        ),
    }))
}

/// A made-up version found (nothing told): `long`, notes longer than the window holds.
pub fn found(long: bool) -> bool {
    let mut notes = String::from("<!-- app -->\n## What's new\n");
    let count = if long { 40 } else { 4 };
    for i in 1..=count {
        notes.push_str(&format!(
            "- Change {i}: the branch list in the top bar, stash and the file badges\n"
        ));
    }
    notes.push_str("<!-- /app -->\n## Download\n");
    let Some(found) = made_up("1.0.0-beta.11", &notes) else { return false };
    state::set(Phase::Available(found), None);
    true
}
