//! PHP language level (the PHP version the code is checked against): the known versions,
//! the version since which newer keywords and functions exist, and detection of
//! the project version from `composer.json`.

use code_intelligence::{DetectedLevel, Project};

/// The known versions, ascending (from the generated stubs: a new PHP release appears here
/// once the stubs are regenerated).
pub fn levels() -> &'static [&'static str] {
    // Kept (a few words) when the stubs are let go.
    static LEVELS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    LEVELS.get_or_init(|| crate::stubs::get().versions.clone())
}

/// Keywords, types and constants introduced after PHP 5.6 (functions and classes: the stubs).
const SINCE: &[(&str, &str)] = &[
    // Keywords and types.
    ("void", "7.1"),
    ("iterable", "7.1"),
    ("object", "7.2"),
    ("fn", "7.4"),
    ("match", "8.0"),
    ("mixed", "8.0"),
    ("enum", "8.1"),
    ("readonly", "8.1"),
    ("never", "8.1"),
    // Constants.
    ("PHP_FLOAT_EPSILON", "7.2"),
    ("PHP_OS_FAMILY", "7.2"),
];

/// The version a keyword, type, function, class or constant appeared in (`None`: always
/// available). Functions, classes and constants come from the stubs.
pub fn since(word: &str) -> Option<&'static str> {
    SINCE
        .iter()
        .find(|(w, _)| *w == word)
        .map(|(_, v)| *v)
        .or_else(|| crate::stubs::get().since(word))
}

/// The project version, in order:
/// 1. `config.platform.php` from `composer.json` (the version declared for production);
/// 2. the lowest version accepted by `require.php` (`^8.2` → `8.2`, `>=7.4 <8.3` → `7.4`);
/// 3. the installed PHP interpreter (new projects, without `composer.json`).
pub fn detect(project: &Project<'_>) -> Option<DetectedLevel> {
    from_composer(project).or_else(|| {
        // Only as found in the background (`warm`): opening a project never waits for PHP.
        let (full, _) = crate::interpreter::known()?;
        Some(DetectedLevel { level: floor(full)?, source: format!("php {full}") })
    })
}

fn from_composer(project: &Project<'_>) -> Option<DetectedLevel> {
    let root = project.root?;
    let json = (project.read)(&root.join("composer.json"))?;
    let composer: serde_json::Value = serde_json::from_str(&json).ok()?;
    let found = |pointer: &str| composer.pointer(pointer).and_then(|v| v.as_str()).and_then(floor);
    if let Some(level) = found("/config/platform/php") {
        return Some(DetectedLevel { level, source: "composer.json · config.platform".into() });
    }
    found("/require/php").map(|level| DetectedLevel { level, source: "composer.json".into() })
}

/// The lowest known version accepted by the constraint.
fn floor(constraint: &str) -> Option<String> {
    let mut lowest: Option<(u32, u32)> = None;
    for alternative in constraint.split("||").flat_map(|a| a.split('|')) {
        // In an alternative, the first version without `<` is its minimum.
        let part = alternative
            .split([' ', ','])
            .map(str::trim)
            .find(|p| !p.is_empty() && !p.starts_with('<') && !p.starts_with("!="));
        let Some(part) = part else { continue };
        let digits = part.trim_start_matches(|c: char| !c.is_ascii_digit());
        let mut numbers = digits.split('.').map(|n| {
            n.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u32>().ok()
        });
        let Some(Some(major)) = numbers.next() else { continue };
        let minor = numbers.next().flatten().unwrap_or(0);
        let version = (major, minor);
        lowest = Some(lowest.map_or(version, |l| l.min(version)));
    }
    let (major, minor) = lowest?;
    // The exact version if known, otherwise the closest one below it.
    levels()
        .iter()
        .rev()
        .find(|l| {
            let mut p = l.split('.').map(|n| n.parse::<u32>().unwrap_or(0));
            (p.next().unwrap_or(0), p.next().unwrap_or(0)) <= (major, minor)
        })
        .or(levels().first())
        .map(|l| (*l).to_owned())
}

#[cfg(test)]
#[path = "../tests/unit/version.rs"]
mod tests;
