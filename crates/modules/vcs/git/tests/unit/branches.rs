use super::*;

fn label(name: &str, kind: RefKind) -> RefLabel {
    RefLabel { name: name.into(), kind }
}

fn shown(rows: &[BranchRow]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            let text = match &r.item {
                BranchItem::All => "all".to_owned(),
                BranchItem::Head => "HEAD".to_owned(),
                BranchItem::Group { label, expanded, .. } => {
                    format!("{label}{}", if *expanded { "/" } else { "+" })
                }
                BranchItem::Ref { label, .. } => label.clone(),
            };
            format!("{}{text}", "  ".repeat(r.depth))
        })
        .collect()
}

#[test]
fn branches_in_folders_by_section() {
    let refs = [
        label("main", RefKind::Branch),
        label("feature/login", RefKind::Branch),
        label("feature/chat/v2", RefKind::Branch),
        label("origin/main", RefKind::Remote),
        label("origin/feature/login", RefKind::Remote),
        label("v1.0", RefKind::Tag),
    ];
    let mut collapsed = HashSet::new();
    let rows = rows(&refs, &collapsed, "");
    let local = i18n::t("git.branches_local");
    let remote = i18n::t("git.branches_remote");
    let tags = i18n::t("git.branches_tags");
    assert_eq!(
        shown(&rows),
        [
            "all".to_owned(),
            "HEAD".into(),
            format!("{local}/"),
            "  feature/".into(),
            "    chat/".into(),
            "      v2".into(),
            "    login".into(),
            "  main".into(),
            format!("{remote}/"),
            "  origin/".into(),
            "    feature/".into(),
            "      login".into(),
            "    main".into(),
            format!("{tags}/"),
            "  v1.0".into(),
        ]
    );
    // A leaf filters by its full name.
    assert!(rows.iter().any(|r| r.item
        == BranchItem::Ref {
            name: "origin/feature/login".into(),
            label: "login".into(),
            kind: RefKind::Remote
        }));
    // A folder closed hides what's in it.
    collapsed.insert("local/feature".to_owned());
    let rows = super::rows(&refs, &collapsed, "");
    assert!(shown(&rows).contains(&"  feature+".to_owned()));
    assert!(!shown(&rows).contains(&"    login".to_owned()));
}
