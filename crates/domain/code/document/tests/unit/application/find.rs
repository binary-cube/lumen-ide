use super::*;

fn query(p: &str) -> SearchQuery {
    SearchQuery { pattern: p.into(), ..Default::default() }
}

#[test]
fn find_next_wraps_and_counts() {
    let mut doc = Document::new("a x a x a");
    let mut f = FindState::default();
    f.set_query(query("a"));
    f.find_next(&mut doc); // the cursor is at 0: the first match is right there
    assert_eq!(doc.selections().primary(), Selection::new(0, 1));
    f.find_next(&mut doc);
    assert_eq!(doc.selections().primary(), Selection::new(4, 5));
    assert_eq!(f.counter(&doc), (2, 3));
    f.find_next(&mut doc);
    f.find_next(&mut doc);
    assert_eq!(doc.selections().primary(), Selection::new(0, 1));
    f.find_prev(&mut doc);
    assert_eq!(doc.selections().primary(), Selection::new(8, 9));
}

#[test]
fn replace_current_then_all_with_single_undo() {
    let mut doc = Document::new("$a + $a + $a");
    let mut f = FindState::default();
    f.set_query(query("$a"));
    f.replacement = "$b".into();
    f.select_from_cursor(&mut doc);
    f.replace_current(&mut doc);
    assert_eq!(doc.buffer().to_string(), "$b + $a + $a");
    assert_eq!(f.replace_all(&mut doc), 2);
    assert_eq!(doc.buffer().to_string(), "$b + $b + $b");
    doc.undo();
    assert_eq!(doc.buffer().to_string(), "$b + $a + $a");
}

#[test]
fn matches_are_recomputed_after_edits() {
    let mut doc = Document::new("ab");
    let mut f = FindState::default();
    f.set_query(query("b"));
    assert_eq!(f.matches(&doc).len(), 1);
    crate::application::commands::insert_text(&mut doc, "b");
    assert_eq!(f.matches(&doc).len(), 2);
}

#[test]
fn invalid_regex_reports_error() {
    let mut f = FindState::default();
    f.set_query(SearchQuery { pattern: "(".into(), regex: true, ..Default::default() });
    assert!(f.error().is_some());
    assert!(f.matches(&Document::new("(")).is_empty());
}

/// After edits (typed, deleted, pasted over lines, several at once, several versions between
/// two looks), the matches worked out on the touched lines only are the whole search's.
#[test]
fn matches_after_edits_are_the_whole_search_s() {
    let queries = [
        query("ab"),
        SearchQuery { pattern: "^a".into(), regex: true, ..Default::default() },
        SearchQuery { pattern: "b$".into(), regex: true, ..Default::default() },
        SearchQuery { pattern: "ab".into(), whole_word: true, ..Default::default() },
        SearchQuery { pattern: "a+".into(), regex: true, ..Default::default() },
    ];
    let pieces = ["a", "b", "ab", " ", "\n", "ab ab\nab", "", "ș"];
    let start: String = (0..400).map(|i| ["ab ", "a\n", "xab\n", "b ", "ab\n"][i % 5]).collect();
    for q in queries {
        let mut doc = Document::new(&start);
        let mut f = FindState::default();
        f.set_query(q.clone());
        let mut seed: u64 = 7;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n.max(1)
        };
        for step in 0..300 {
            let edits = 1 + next(3);
            let mut list = Vec::new();
            let len = doc.buffer().len();
            let mut at = 0;
            for _ in 0..edits {
                let a = doc.buffer().floor_char_boundary((at + next(len / 3 + 1)).min(len));
                let b = doc.buffer().floor_char_boundary((a + next(6)).min(len));
                list.push(Edit::new(a..b, pieces[next(pieces.len())].to_owned()));
                at = b + 1;
                if at >= len {
                    break;
                }
            }
            doc.apply(Transaction::new(list), None, EditKind::Command);
            if step % 4 == 3 {
                continue; // a look skipped: several versions at once next time
            }
            let whole = Matcher::new(&q).unwrap().unwrap().find_all(doc.buffer());
            assert_eq!(f.matches(&doc), whole.as_slice(), "{q:?} at step {step}");
        }
    }
}

/// A large document's whole search goes elsewhere: the matches wait for it (nothing
/// searched here meanwhile), the selection asked meanwhile is made when it comes, an answer
/// for an older query is dropped; edits after it are searched here on their lines.
#[test]
fn a_whole_search_is_handed_out_and_its_answer_taken() {
    let mut doc = Document::new(&"ab x ab\n".repeat(50));
    let mut f = FindState::default();
    f.set_query(query("ab"));
    assert!(f.whole_search(&doc, usize::MAX).is_none(), "a small document: searched here");
    let old = f.whole_search(&doc, 10).expect("handed out");
    assert!(f.whole_search(&doc, 10).is_none(), "one out at a time");
    f.set_query(query("x"));
    let job = f.whole_search(&doc, 10).expect("for the new query");
    assert!(f.matches(&doc).is_empty() && f.is_searching(), "waiting, nothing searched here");
    f.select_from_cursor(&mut doc);
    assert_eq!(doc.selections().primary(), Selection::cursor(0), "made when it comes");
    assert!(!f.found(old.generation, old.version, old.run()), "the older query's: dropped");
    assert!(f.is_searching());
    assert!(f.found(job.generation, job.version, job.run()), "the selection to make now");
    f.select_from_cursor(&mut doc);
    assert_eq!(doc.selections().primary(), Selection::new(3, 4));
    assert_eq!(f.matches(&doc).len(), 50);
    document_edit(&mut doc, 0, "x ");
    assert!(f.whole_search(&doc, 10).is_none(), "an edit: its lines searched here");
    assert_eq!(f.matches(&doc).len(), 51);
    // Going to the next match doesn't wait for a search that's out.
    f.set_query(query("ab"));
    assert!(f.whole_search(&doc, 10).is_some());
    f.find_next(&mut doc);
    assert!(!f.is_searching() && f.matches(&doc).len() == 100);
}

fn document_edit(doc: &mut Document, at: usize, text: &str) {
    doc.apply(Transaction::single(at..at, text.to_owned()), None, EditKind::Command);
}
