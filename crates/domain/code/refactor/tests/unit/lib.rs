use super::*;

#[test]
fn whole_words_only() {
    let text = "User $user UserRepository user_id ->user( $x->user";
    let found: Vec<&str> =
        find_word(text, "user", NameStyle::CODE).iter().map(|r| &text[r.clone()]).collect();
    // `$user` is the variable named `user`: the property / method search finds it too and
    // the resolver tells them apart.
    assert_eq!(found, ["user", "user", "user"]);
    let php = NameStyle::CODE.with_sigil(Some('$'));
    let vars = find_word("$total = $total + $totals;", "$total", php);
    assert_eq!(vars, [0..6, 9..15]);
    let css = find_word(".card .card-body .card", "card", NameStyle::CSS);
    assert_eq!(css, [1..5, 18..22]);
}

#[test]
fn applies_edits_and_refuses_stale_ones() {
    let edits = [
        TextEdit { range: 0..4, original: "User".into(), text: "Customer".into() },
        TextEdit { range: 9..13, original: "User".into(), text: "Customer".into() },
    ];
    assert_eq!(apply_edits("User and User.", &edits).unwrap(), "Customer and Customer.");
    assert_eq!(apply_edits("Us", &edits), Err(EditError::Stale));
    // The text changed since the plan: the name isn't where it was.
    assert_eq!(apply_edits("Usr  and User.", &edits), Err(EditError::Stale));
    let overlapping = [
        TextEdit { range: 2..5, original: String::new(), text: String::new() },
        TextEdit { range: 3..4, original: String::new(), text: String::new() },
    ];
    assert_eq!(apply_edits("abcdef", &overlapping), Err(EditError::Stale));
}

#[test]
fn plan_counts_and_renames_files_with_the_name() {
    let mut plan = Plan {
        files: vec![FilePlan {
            path: "/p/a.php".into(),
            occurrences: vec![
                Occurrence {
                    range: 0..4,
                    original: "User".into(),
                    replacement: "User".into(),
                    certainty: Certainty::Certain,
                    chosen: true,
                    line: 0,
                    line_text: "User".into(),
                    in_line: 0..4,
                },
                Occurrence {
                    range: 10..14,
                    original: "User".into(),
                    replacement: "User".into(),
                    certainty: Certainty::Uncertain,
                    chosen: false,
                    line: 0,
                    line_text: String::new(),
                    in_line: 0..0,
                },
            ],
        }],
        renames: vec![FileRename {
            from: "/p/User.php".into(),
            to: "/p/User.php".into(),
            chosen: true,
        }],
    };
    plan.set_replacement("Customer");
    assert_eq!(plan.chosen(), (1, 1));
    assert_eq!(plan.renames[0].to, std::path::PathBuf::from("/p/Customer.php"));
    let edits = plan.edits();
    assert_eq!(
        edits[0].edits,
        [TextEdit { range: 0..4, original: "User".into(), text: "Customer".into() }]
    );
    plan.choose_all(false);
    assert_eq!(plan.chosen(), (0, 0));
    assert!(!plan.renames[0].chosen);
    plan.choose_file(0, true);
    assert_eq!(plan.chosen(), (2, 1));
    let (line, text, within) = line_of("a\n  User x\nb", &(4..8));
    assert_eq!((line, text.as_str(), within), (1, "  User x", 2..6));
    assert!(valid_name("Customer_2", NameStyle::CODE) && !valid_name("2x", NameStyle::CODE));
    assert!(valid_name("card-body", NameStyle::CSS) && !valid_name("a b", NameStyle::CSS));
}
