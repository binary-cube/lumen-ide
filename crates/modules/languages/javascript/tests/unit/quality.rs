use super::*;

/// ESLint's JSON: its messages, warnings (1) and errors (2), the rule, fixable when it has a
/// fix.
#[test]
fn eslint_reports_are_read() {
    let report = r#"[{"filePath":"/p/src/a.js","messages":[
        {"ruleId":"no-unused-vars","severity":2,"message":"'x' is defined but never used.","line":3,"column":7},
        {"ruleId":"semi","severity":1,"message":"Missing semicolon.","line":5,"column":12,"fix":{"range":[40,40],"text":";"}}]}]"#;
    let output = ToolOutput { code: Some(1), stdout: report.into(), stderr: String::new() };
    let problems = Eslint.problems(&output);
    assert_eq!(problems.len(), 2);
    assert_eq!((problems[0].line, problems[0].column), (2, Some(6)));
    assert_eq!(problems[0].severity, Severity::Error);
    assert_eq!(problems[1].severity, Severity::Warning);
    assert!(problems[1].fixable && !problems[0].fixable);
    assert_eq!(
        Prettier.args(&Target::Project, Path::new("/p")),
        ["--write", "--log-level=warn", "."]
    );
}

/// A file is given on stdin, named for the tools' configs; the project is their folder.
#[test]
fn a_file_goes_on_stdin_and_the_package_runs_on_node() {
    let file = Target::File("src/a.ts".into());
    let root = Path::new("/p");
    assert_eq!(Prettier.args(&file, root), ["--stdin-filepath", "src/a.ts"]);
    assert_eq!(
        Eslint.args(&file, root),
        ["--format=json", "--stdin", "--stdin-filename", "src/a.ts"]
    );
    assert_eq!(Eslint.args(&Target::Project, root), ["--format=json", "."]);
    assert!(Prettier.stdin() && Eslint.stdin());
    assert_eq!(Eslint.scripts(), ["node_modules/eslint/bin/eslint.js"]);
    assert_eq!(Prettier.scripts()[0], "node_modules/prettier/bin/prettier.cjs");
}
