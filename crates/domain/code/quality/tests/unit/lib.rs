use super::*;

fn out(stdout: &str) -> ToolOutput {
    ToolOutput { code: Some(1), stdout: stdout.into(), stderr: String::new() }
}

/// Lines `file:line:column: message` and `file:line: message` read as problems (0-based),
/// warnings told apart, other lines skipped, Windows drives kept in the path.
#[test]
fn problems_read_from_lines() {
    let problems = read_lines(
        &out("src/A.php:12:5: Missing semicolon\n\
              Checking 3 files...\n\
              src/B.php:3: warning: unused variable $x\n\
              C:\\proj\\c.js:1:2: error: bad"),
        "script",
    );
    assert_eq!(problems.len(), 3);
    assert_eq!((problems[0].line, problems[0].column), (11, Some(4)));
    assert_eq!(problems[0].message, "Missing semicolon");
    assert_eq!(problems[0].severity, Severity::Error);
    assert_eq!(problems[1].severity, Severity::Warning);
    assert_eq!(
        (problems[1].column, problems[1].message.as_str()),
        (None, "warning: unused variable $x")
    );
    assert_eq!(problems[2].path, PathBuf::from("C:\\proj\\c.js"));
    assert_eq!(problems[2].tool, "script");
}
