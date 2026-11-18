use super::*;

/// Stylelint's JSON, on stdout or stderr: its warnings with their rule and severity.
#[test]
fn stylelint_reports_are_read() {
    let report = r##"[{"source":"/p/a.css","warnings":[{"line":2,"column":3,"rule":"color-no-invalid-hex","severity":"error","text":"Unexpected invalid hex color \"#ffz\" (color-no-invalid-hex)"}]}]"##;
    for output in [
        ToolOutput { code: Some(2), stdout: report.into(), stderr: String::new() },
        ToolOutput { code: Some(2), stdout: String::new(), stderr: report.into() },
    ] {
        let problems = Stylelint.problems(&output);
        assert_eq!(problems.len(), 1);
        assert_eq!((problems[0].line, problems[0].column), (1, Some(2)));
        assert_eq!(problems[0].rule.as_deref(), Some("color-no-invalid-hex"));
    }
}
