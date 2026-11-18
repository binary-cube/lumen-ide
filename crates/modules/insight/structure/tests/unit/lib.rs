use super::*;

#[test]
fn header_runs_cover_the_text() {
    let red = Rgba::rgb(0xff0000);
    let r = runs("function stop()", &[(0..8, red), (9..13, red)]);
    let joined: String = r.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(joined, "function stop()");
    assert_eq!(r[0], ("function".to_owned(), Some(red)));
    assert_eq!(r[1].1, None);
}

#[test]
fn header_wraps_after_a_space_keeping_colors() {
    let red = Rgba::rgb(0xff0000);
    let runs = vec![("function".to_owned(), Some(red)), (" stop(int $speed)".to_owned(), None)];
    let lines = wrap(&runs, 16);
    let text: Vec<String> =
        lines.iter().map(|line| line.iter().map(|(s, _)| s.as_str()).collect()).collect();
    assert_eq!(text, ["function ", "stop(int $speed)"]);
    assert_eq!(lines[0][0], ("function".to_owned(), Some(red)));
}
