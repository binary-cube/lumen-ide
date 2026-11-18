use super::*;

#[test]
fn a_path_reads_by_its_name_first() {
    assert_eq!(short_path("/work/p/src/Auth/a.php"), "a.php — src/Auth");
    assert_eq!(short_path("a.php"), "a.php");
}

/// A message by paragraphs (an empty line kept); a tool its line and one a change, linked,
/// with the lines the change adds and removes; laid out for the revision given.
#[test]
fn items_become_lines() {
    let mut text = TextSystem::new();
    let font = Font::new(text_layout::FontFamily::SansSerif, 14.0);
    let message = Item::Message { author: Author::Agent, text: "one\n\ntwo".into() };
    let laid = lay(&message, (0, 7), &mut text, &font, 400.0);
    assert_eq!(laid.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["one", "", "two"]);
    assert_eq!(laid.revision, 7);
    let tool = Item::Tool(acp::Tool {
        id: "t".into(),
        title: "Edit".into(),
        kind: "edit".into(),
        status: ToolStatus::Done,
        output: String::new(),
        diffs: vec![acp::Diff::new("/p/a.php".into(), Some("a\nb\nc".into()), "a\nB\nc\nd".into())],
        paths: Vec::new(),
    });
    let laid = lay(&tool, (3, 1), &mut text, &font, 400.0);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[0].icon, Some("check"));
    assert_eq!(laid.lines[1].link, Some(Link::Diff { item: 3, diff: 0 }));
    assert_eq!(laid.lines[1].text, "a.php — /p  +2 −1");
    let user = Item::Message { author: Author::User, text: "hi".into() };
    assert!(lay(&user, (0, 1), &mut text, &font, 400.0).bubble);
}

/// A message growing chunk by chunk, laid out on what it was, gives the lines it gives laid
/// out whole: paragraphs ending mid-chunk, empty ones, trailing spaces and breaks, `\r\n`,
/// long ones wrapped, another author's message, a tool after it.
#[test]
fn a_growing_message_is_laid_out_as_a_whole_one() {
    let mut text = TextSystem::new();
    let font = Font::new(text_layout::FontFamily::SansSerif, 14.0);
    let long = "word ".repeat(60);
    let chunks = [
        "Hel",
        "lo",
        " there\n",
        "\n",
        "  ",
        "\n",
        "second ",
        "paragraph",
        "\r\n",
        "third\n\n\n",
        "  \n",
        &long,
        "\n",
        &long,
        "é",
        "ü\nö",
        " ",
        "\n",
        "",
        "end",
    ];
    for author in [Author::Agent, Author::User, Author::Thought] {
        let mut said = String::new();
        let mut laid: Option<Laid> = None;
        for (n, chunk) in chunks.iter().enumerate() {
            said.push_str(chunk);
            let item = Item::Message { author, text: said.clone() };
            let at = (2, n as u64 + 1);
            let grown = match laid.take() {
                Some(old) => lay_on(old, &item, at, &mut text, &font, 300.0),
                None => lay(&item, at, &mut text, &font, 300.0),
            };
            assert_eq!(grown, lay(&item, at, &mut text, &font, 300.0), "{author:?} {said:?}");
            laid = Some(grown);
        }
        // Another author, or another item, laid out anew.
        let other = Item::Message { author: Author::Agent, text: "x".into() };
        let author_changed =
            lay_on(laid.clone().unwrap(), &other, (2, 99), &mut text, &font, 300.0);
        assert_eq!(author_changed, lay(&other, (2, 99), &mut text, &font, 300.0));
        let plan = Item::Plan(vec![acp::Step { text: "s".into(), done: true, running: false }]);
        let planned = lay_on(laid.unwrap(), &plan, (2, 100), &mut text, &font, 300.0);
        assert_eq!(planned, lay(&plan, (2, 100), &mut text, &font, 300.0));
    }
}
