use super::*;
use code_intelligence::{SymbolIndex, SymbolKind};
use std::path::Path;

#[test]
fn a_duplicate_key_is_a_problem_named_after_it() {
    let index = SymbolIndex::default();
    let types = Types { index: &index, path: Path::new(".env") };
    let checked = DotenvIntelligence.check("A=1\nA=2\n", &types);
    assert_eq!(checked.problems.len(), 1);
    let problem = &checked.problems[0];
    assert_eq!(
        (problem.kind, problem.name.as_str(), problem.range.clone()),
        ("env_duplicate_key", "A", 4..5)
    );
}

#[test]
fn the_symbols_and_folds_follow_the_registered_rules() {
    // Without rules registered, the comments alone make the sections.
    let text = "# Mail\nMAIL_HOST=smtp\nMAIL_PORT=25\n";
    let file = DotenvIntelligence.extract(text);
    let kinds: Vec<(SymbolKind, &str)> =
        file.symbols.iter().map(|s| (s.kind, s.name.as_ref())).collect();
    assert_eq!(
        kinds,
        [
            (SymbolKind::Section, "Mail"),
            (SymbolKind::Constant, "MAIL_HOST"),
            (SymbolKind::Constant, "MAIL_PORT")
        ]
    );
    let folds = syntax_ts::FoldProvider::folds(&SectionFolds, text);
    assert_eq!(folds.iter().map(|f| (f.start_line, f.end_line)).collect::<Vec<_>>(), [(0, 2)]);
}
