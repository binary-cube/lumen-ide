use super::*;

fn out(stdout: &str) -> ToolOutput {
    ToolOutput { code: Some(2), stdout: stdout.into(), stderr: String::new() }
}

/// phpcs's JSON report: each message a problem (0-based), warnings and errors told apart,
/// its sniff as the rule, fixable when phpcbf can.
#[test]
fn phpcs_reports_are_read() {
    let report = r#"{"totals":{"errors":1,"warnings":1},"files":{"/p/src/A.php":{"errors":1,"warnings":1,"messages":[
        {"message":"Missing file doc comment","source":"PEAR.Commenting.FileComment.Missing","severity":5,"fixable":false,"type":"ERROR","line":2,"column":1},
        {"message":"Line exceeds 120 characters","source":"Generic.Files.LineLength.TooLong","severity":5,"fixable":true,"type":"WARNING","line":14,"column":121}]}}}"#;
    let problems = Phpcs.problems(&out(report));
    assert_eq!(problems.len(), 2);
    assert_eq!((problems[0].line, problems[0].column), (1, Some(0)));
    assert_eq!(problems[0].severity, Severity::Error);
    assert_eq!(problems[0].rule.as_deref(), Some("PEAR.Commenting.FileComment.Missing"));
    assert_eq!(problems[1].severity, Severity::Warning);
    assert!(problems[1].fixable);
    assert_eq!(problems[1].path, PathBuf::from("/p/src/A.php"));
    assert!(Phpcs.problems(&out("not json")).is_empty());
}

/// PHPStan's JSON: its messages, with their identifier.
#[test]
fn phpstan_reports_are_read() {
    let report = r#"{"totals":{"errors":0,"file_errors":1},"files":{"/p/src/A.php":{"errors":1,"messages":[
        {"message":"Undefined variable: $x","line":7,"ignorable":true,"identifier":"variable.undefined"}]}},"errors":[]}"#;
    let problems = Phpstan.problems(&out(report));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].line, 6);
    assert_eq!(problems[0].rule.as_deref(), Some("variable.undefined"));
}

/// PHP CS Fixer in `--dry-run`: each file it would change, a problem a formatter repairs.
#[test]
fn php_cs_fixer_dry_runs_are_read() {
    let report = r#"{"files":[{"name":"src/A.php","appliedFixers":["braces","single_quote"]}],"time":{"total":0.1}}"#;
    let problems = CsFixerCheck.problems(&out(report));
    assert_eq!(problems.len(), 1);
    assert!(problems[0].fixable);
    let message = i18n::tr!("quality.not_formatted", rules = "braces, single_quote");
    assert_eq!(problems[0].message, message);
    assert_eq!(
        CsFixer.args(&Target::File("src/A.php".into()), Path::new("/p")),
        ["fix", "--quiet", "src/A.php"]
    );
}
