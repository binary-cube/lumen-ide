use super::*;

fn run(filter: &mut EraseFilter, input: &[u8], honor: bool) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut asked = 0;
    filter.feed(input, &mut out, &mut || {
        asked += 1;
        honor
    });
    (out, asked)
}

#[test]
fn drops_only_the_history_erase_when_not_honored() {
    let mut f = EraseFilter::default();
    let (out, asked) = run(&mut f, b"a\x1b[2J\x1b[3J\x1b[Hb", false);
    // The screen erase beside it quiet (nothing pushed into the history).
    assert_eq!(out, b"a\x1b[H\x1b[J\x1b[Hb");
    assert_eq!(asked, 1);
    assert!(f.idle());
}

#[test]
fn keeps_it_when_honored() {
    let mut f = EraseFilter::default();
    let input = b"\x1b[2J\x1b[3J\x1b[H";
    assert_eq!(run(&mut f, input, true).0, input);
}

#[test]
fn finds_it_split_across_reads_at_every_byte() {
    let input = b"x\x1b[2J\x1b[3J\x1b[1;31my\x1b]0;t\x07z";
    for split in 0..=input.len() {
        let mut f = EraseFilter::default();
        let mut out = run(&mut f, &input[..split], false).0;
        out.extend(run(&mut f, &input[split..], false).0);
        assert_eq!(out, b"x\x1b[H\x1b[J\x1b[1;31my\x1b]0;t\x07z", "split at {split}");
        assert!(f.idle());
    }
}

#[test]
fn leaves_every_other_sequence_alone() {
    let mut f = EraseFilter::default();
    for seq in [
        &b"\x1b[J"[..],
        b"\x1b[0J",
        b"\x1b[2J",
        b"\x1b[13J",
        b"\x1b[?3J",
        b"\x1b[3K",
        b"\x1b[3 J",
        b"\x1bc",
        b"\x1b\x1b[3",
        b"\x1b[3\nJ",
    ] {
        // What's still held (an unfinished one) is let out by what follows.
        let (mut out, asked) = run(&mut f, seq, false);
        out.extend(run(&mut f, b"\x07", false).0);
        assert_eq!(out, [seq, b"\x07"].concat(), "{seq:?}");
        assert_eq!(asked, 0, "{seq:?}");
        f = EraseFilter::default();
    }
}

#[test]
fn reads_the_first_parameter_as_the_emulator_does() {
    for seq in [&b"\x1b[03J"[..], b"\x1b[3;1J", b"\x1b[3:2J"] {
        let mut f = EraseFilter::default();
        assert_eq!(run(&mut f, seq, false), (Vec::new(), 1), "{seq:?}");
    }
}

#[test]
fn an_overlong_sequence_passes_unchanged() {
    let mut f = EraseFilter::default();
    let mut seq = b"\x1b[".to_vec();
    seq.extend(std::iter::repeat_n(b'1', 40));
    seq.push(b'm');
    assert_eq!(run(&mut f, &seq, false).0, seq);
}

#[test]
fn the_policies() {
    let shell = || Foreground::Shell;
    let clear = || Foreground::Program(Duration::from_millis(30));
    let agent = || Foreground::Program(Duration::from_secs(90));
    let unknown = || Foreground::Unknown;
    use HistoryClear::*;
    assert!(honors(Standard, agent));
    assert!(!honors(KeepAlways, shell));
    assert!(!honors(KeepAlways, clear));
    assert!(honors(KeepForRunningPrograms, shell));
    assert!(honors(KeepForRunningPrograms, clear));
    assert!(honors(KeepForRunningPrograms, unknown));
    assert!(!honors(KeepForRunningPrograms, agent));
    // The processes aren't looked at when the policy decides alone.
    assert!(honors(Standard, || unreachable!()));
    for p in [Standard, KeepForRunningPrograms, KeepAlways] {
        assert_eq!(HistoryClear::from_u8(p.to_u8()), p);
    }
}

/// The screen erase beside a dropped history erase is quiet — before it, or after it with
/// only the cursor moved between (`clear`'s order) —, split anywhere; alone, or beside an
/// honored one, or with output between, it's let out as it is; held at the end of what was
/// read, [`EraseFilter::flush`] lets it out.
#[test]
fn the_screen_erase_beside_a_dropped_one_is_quiet() {
    for (input, honor, expected) in [
        (&b"\x1b[2J\x1b[3J\x1b[Hf"[..], false, &b"\x1b[H\x1b[J\x1b[Hf"[..]),
        (b"\x1b[3J\x1b[H\x1b[2Jf", false, b"\x1b[H\x1b[H\x1b[Jf"),
        (b"\x1b[H\x1b[2J\x1b[3Jf", false, b"\x1b[H\x1b[H\x1b[Jf"),
        (b"\x1b[2J\x1b[3J\x1b[Hf", true, b"\x1b[2J\x1b[3J\x1b[Hf"),
        (b"\x1b[2Jf", false, b"\x1b[2Jf"),
        (b"\x1b[2Jf\x1b[3J", false, b"\x1b[2Jf"),
        (b"\x1b[3Jf\x1b[2Jg", false, b"f\x1b[2Jg"),
        (b"\x1b[2J\x1b[2J\x1b[3Jf", false, b"\x1b[2J\x1b[H\x1b[Jf"),
    ] {
        for split in 0..=input.len() {
            let mut f = EraseFilter::default();
            let mut out = run(&mut f, &input[..split], honor).0;
            out.extend(run(&mut f, &input[split..], honor).0);
            f.flush(&mut out);
            assert_eq!(out, expected, "{input:?} split at {split}");
            assert!(f.idle());
        }
    }
    let mut f = EraseFilter::default();
    let (out, _) = run(&mut f, b"a\x1b[2J", false);
    assert_eq!(out, b"a", "held: what follows decides");
    assert!(!f.idle() && f.erase_held());
    let mut out = Vec::new();
    f.flush(&mut out);
    assert_eq!(out, b"\x1b[2J");
}

/// What erases the history is told, whatever came after it in the same read: an `ESC[3J`
/// let through, a full reset (`ESC c`); never a dropped one, nor another sequence.
#[test]
fn the_history_erased_is_told() {
    for (input, honor, erased) in [
        (&b"a\x1b[3Jb"[..], true, true),
        (b"a\x1b[3Jb", false, false),
        (b"\x1b[3J\x1b[2J\x1b[3J", true, true),
        (b"x\x1bcy", false, true),
        (b"\x1b[c\x1b[2J\x1b(B", true, false),
    ] {
        for split in 0..=input.len() {
            let mut f = EraseFilter::default();
            run(&mut f, &input[..split], honor);
            run(&mut f, &input[split..], honor);
            assert_eq!(f.take_seen().erased, erased, "{input:?} split at {split}");
            assert!(!f.take_seen().erased, "told once");
        }
    }
}
