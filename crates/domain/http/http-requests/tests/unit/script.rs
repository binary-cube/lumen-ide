//! A script's run: passed only when nothing failed.

use super::*;

#[test]
fn a_run_passes_only_when_every_test_does_and_nothing_failed() {
    let mut run = ScriptRun::default();
    assert!(run.passed(), "no tests: nothing failed");
    run.tests.push(TestResult { name: "a".into(), failure: None });
    assert!(run.passed());
    run.tests.push(TestResult { name: "b".into(), failure: Some("no".into()) });
    assert!(!run.passed());
    let broken = ScriptRun { error: Some("boom".into()), ..ScriptRun::default() };
    assert!(!broken.passed());
}
