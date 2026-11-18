use super::*;

#[test]
fn identifiers_are_split() {
    let t = terms("getUserName(user_id, HTTPServer2)");
    for w in
        ["get", "user", "name", "getusername", "id", "user_id", "http", "server", "httpserver2"]
    {
        assert!(t.contains(&w.to_owned()), "{w} missing from {t:?}");
    }
}

#[test]
fn chunks_follow_blank_lines_and_size() {
    let text = (0..100)
        .map(|i| if i % 10 == 9 { String::new() } else { format!("line {i}") })
        .collect::<Vec<_>>()
        .join("\n");
    let parts = chunks(&text);
    assert!(parts.iter().all(|(s, e, _)| e - s < CHUNK_LINES as u32));
    assert_eq!(parts.first().map(|p| p.0), Some(0));
    assert!(parts.windows(2).all(|w| w[0].1 < w[1].0), "no overlaps");
}

/// A test "model": the vector is the histogram of a few keywords.
struct Toy;
impl Embedder for Toy {
    fn embed(&self, text: &str) -> Vec<f32> {
        let t = text.to_lowercase();
        let mut v: Vec<f32> = ["mail", "send", "price", "invoice"]
            .iter()
            .map(|w| t.matches(w).count() as f32)
            .collect();
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        v.iter_mut().for_each(|x| *x /= norm);
        v
    }
    fn dims(&self) -> usize {
        4
    }
}

#[test]
fn lexical_and_semantic_search() {
    let mut index = SearchIndex::default();
    index.set_file(
        "app/Mailer.php",
        "class Mailer {\n  function sendConfirmation($user) { mail($user->email); }\n}",
        Some(&Toy),
    );
    index.set_file(
        "app/Invoice.php",
        "class Invoice {\n  function totalPrice() { return 1; }\n}",
        Some(&Toy),
    );
    let hits = index.search("sendConfirmation", None, 5);
    assert_eq!(hits.first().map(|h| h.path.as_str()), Some("app/Mailer.php"));
    // No words in common, only the vector finds the matching chunk.
    let hits = index.search("cost", Some(&Toy.embed("price invoice")), 5);
    assert_eq!(hits.first().map(|h| h.path.as_str()), Some("app/Invoice.php"));
    index.remove_file("app/Mailer.php");
    assert!(index.search("sendConfirmation", None, 5).is_empty());
}

#[test]
fn filters_and_related() {
    let mut index = SearchIndex::default();
    index.set_file("app/Mailer.php", "function sendMail() { mail(); }", Some(&Toy));
    index.set_file("web/mail.js", "function sendMail() { fetch(); }", Some(&Toy));
    index.set_file("app/Invoice.php", "function totalPrice() { return 1; }", Some(&Toy));
    let mut query = SearchQuery::new("sendMail", 5);
    query.extensions = vec!["js".into()];
    let hits = index.query(&query, None);
    assert_eq!(hits.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(), ["web/mail.js"]);
    query.extensions.clear();
    query.paths = vec!["app/".into()];
    assert!(index.query(&query, None).iter().all(|h| h.path.starts_with("app/")));
    let related = index.related("app/Mailer.php", 0, 1);
    assert_eq!(related.first().map(|h| h.path.as_str()), Some("web/mail.js"));
}

/// Files prepared and applied together give the index that adding them one by one gives;
/// removing several at once, the one removing them one by one gives.
#[test]
fn a_batch_of_files_is_the_same_as_one_by_one() {
    let files = [
        ("a.php", "function sendMail() {}\nfunction readMail() {}\n"),
        ("b.php", "class UserController { function show() {} }\n"),
        ("c.php", "function sendInvoice() { sendMail(); }\n"),
    ];
    let mut one = SearchIndex::default();
    for (path, text) in files {
        one.set_file(path, text, None);
    }
    let mut batch = SearchIndex::default();
    batch.apply(&[], files.iter().map(|(p, t)| SearchIndex::prepare_file(p, t, None)).collect());
    let hits = |i: &SearchIndex| {
        i.search("send mail", None, 10).into_iter().map(|h| h.path).collect::<Vec<_>>()
    };
    assert_eq!(hits(&one), hits(&batch));
    assert_eq!(one.len(), batch.len());
    one.remove_file("a.php");
    one.remove_file("c.php");
    batch.remove_files(&HashSet::from(["a.php", "c.php"]));
    assert_eq!(hits(&one), hits(&batch));
    assert_eq!(batch.len(), 1);
    // Replaced and removed in the same batch.
    batch.apply(
        &["b.php"],
        vec![SearchIndex::prepare_file("a.php", "function sendMail() {}", None)],
    );
    assert_eq!(hits(&batch), ["a.php"]);
    assert_eq!(batch.len(), 1);
}

/// The vectors are computed file by file; stopped meanwhile (the project closing), the rest
/// isn't read and nothing is given back.
#[test]
fn embeddings_stop_between_files() {
    let mut index = SearchIndex::default();
    index.set_file("a.php", "function sendMail() {}", None);
    index.set_file("b.php", "function price() {}", None);
    let read = std::cell::RefCell::new(Vec::new());
    let reader = |path: &str| {
        read.borrow_mut().push(path.to_owned());
        Some("function x() {}".to_owned())
    };
    let all = index.embeddings(&Toy, &reader, &|| false);
    assert_eq!(all.map(|v| v.len()), Some(2));
    read.borrow_mut().clear();
    let stopped = index.embeddings(&Toy, &reader, &|| !read.borrow().is_empty());
    assert_eq!(stopped, None);
    assert_eq!(read.borrow().len(), 1, "the second file wasn't read");
}
