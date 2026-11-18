use super::platform::word;

fn block(argc: i32, path: &str, rest: &[&str], padding: usize) -> Vec<u8> {
    let mut b = argc.to_ne_bytes().to_vec();
    b.extend_from_slice(path.as_bytes());
    b.extend(std::iter::repeat_n(0, 1 + padding));
    for s in rest {
        b.extend_from_slice(s.as_bytes());
        b.push(0);
    }
    b
}

/// The word after the program's path and its padding, nothing after it.
#[test]
fn the_first_word_comes_after_the_path() {
    let b = block(2, "/usr/bin/sleep", &["my-tool", "30", "HOME=/x"], 5);
    assert_eq!(word(&b).as_deref(), Some("my-tool"));
}

/// An empty `argv[0]` reads as the next argument: one that looks like an argument isn't taken.
#[test]
fn an_argument_is_never_taken_for_the_name() {
    for secret in ["--token=abc", "https://user:pw@host", "KEY=value", "me@host"] {
        let b = block(2, "/usr/bin/tool", &[secret, "PATH=/bin"], 3);
        assert_eq!(word(&b), None, "{secret}");
    }
    assert_eq!(word(&block(0, "/bin/x", &[], 0)), None, "no arguments");
    assert_eq!(word(&[1, 0]), None, "a cut block");
}
