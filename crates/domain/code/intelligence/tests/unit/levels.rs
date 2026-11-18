use super::*;
use crate::{CompletionContext, DetectedLevel, FileSymbols, Target};

/// A language with versions; the project says `7.4`.
#[derive(Debug)]
struct Versions;

impl LanguageIntelligence for Versions {
    fn language(&self) -> LanguageId {
        LanguageId::new("php")
    }
    fn extensions(&self) -> &[&'static str] {
        &["php"]
    }
    fn extract(&self, _: &str) -> FileSymbols {
        FileSymbols::default()
    }
    fn context(&self, _: &str, offset: usize) -> CompletionContext {
        CompletionContext {
            replace: offset..offset,
            prefix: String::new(),
            kind: crate::ContextKind::None,
        }
    }
    fn target(&self, _: &str, _: usize) -> Option<Target> {
        None
    }
    fn locals(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
    fn levels(&self) -> &[&'static str] {
        &["7.4", "8.0", "8.3"]
    }
    fn detect_level(&self, _: &Project<'_>) -> Option<DetectedLevel> {
        Some(DetectedLevel { level: "7.4".into(), source: "composer.json".into() })
    }
}

fn project() -> Project<'static> {
    Project::none()
}

/// The user's choice, else what the project says, else the newest; a file's own level
/// over the project's; what's kept comes back as it went (a level no longer known is let
/// go).
#[test]
fn the_choice_then_the_project_then_the_newest() {
    let languages: Vec<Arc<dyn LanguageIntelligence>> = vec![Arc::new(Versions)];
    let php = LanguageId::new("php");
    let mut levels = LanguageLevels::new(&KeptLevels::default(), &languages, &project());
    assert_eq!(LanguageLevels::get(&levels.chosen, &php), Some("7.4"), "detected");
    assert!(levels.is_auto(&php));
    levels.set(&languages, &php, Some("8.3"));
    assert_eq!(LanguageLevels::get(&levels.chosen, &php), Some("8.3"));
    levels.set_file("legacy/a.php", &php, Some("7.4"));
    assert_eq!(levels.for_file(Some("legacy/a.php")), [(php.clone(), "7.4".to_owned())]);
    assert_eq!(levels.for_file(Some("b.php")), [(php.clone(), "8.3".to_owned())]);
    let mut kept = levels.kept();
    assert_eq!(kept.levels, [("php".to_owned(), "8.3".to_owned())]);
    kept.files.push(("old.php".into(), "php".into(), "5.6".into()));
    let again = LanguageLevels::new(&kept, &languages, &project());
    assert_eq!(again.kept().files, [("legacy/a.php".into(), "php".into(), "7.4".into())]);
    assert!(!again.is_auto(&php));
}
