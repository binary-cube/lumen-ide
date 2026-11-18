use super::*;

const STYLES: [FuzzyStyle; 3] = [
    FuzzyStyle {
        separators: &['_', '-', '\\', '$'],
        consecutive_bonus: 8,
        boundary_bonus: 6,
        gap_penalty: false,
        prefer_boundaries: false,
    },
    FuzzyStyle {
        separators: &['/', '.', '_', '-'],
        consecutive_bonus: 5,
        boundary_bonus: 10,
        gap_penalty: true,
        prefer_boundaries: false,
    },
    FuzzyStyle {
        separators: &['/', '.', '_', '-'],
        consecutive_bonus: 5,
        boundary_bonus: 10,
        gap_penalty: true,
        prefer_boundaries: true,
    },
];

fn lowered(query: &str) -> Vec<char> {
    query.chars().flat_map(char::to_lowercase).collect()
}

#[test]
fn the_score_alone_is_the_matchs_score() {
    let texts = [
        "array_map",
        "ArrayIterator",
        "strlen",
        "str_contains",
        "App\\Module3\\Service7",
        "src/Module3/Service7.php",
        "getUserName",
        "ÉtéÀ_ü",
        "",
        "x",
        "aaaa_bbbb_aaaa",
    ];
    let queries = [
        "", "a", "am", "arr", "s", "sc", "str", "strl", "mod7", "serv", "gun", "été", "ü", "ab",
        "xx", "ba",
    ];
    for style in &STYLES {
        for text in texts {
            for query in queries {
                let q = lowered(query);
                assert_eq!(
                    fuzzy_score(text, &q, style),
                    fuzzy_match(text, &q, style).map(|(s, _)| s),
                    "{text:?} for {query:?} ({style:?})"
                );
            }
        }
    }
}

#[test]
fn a_query_that_is_not_a_subsequence_matches_nothing() {
    let q = lowered("xyz");
    assert_eq!(fuzzy_match("x_y", &q, &STYLES[0]), None);
    assert_eq!(fuzzy_match("zyx", &q, &STYLES[2]), None);
    assert!(fuzzy_match("x_y_z", &q, &STYLES[0]).is_some());
}
