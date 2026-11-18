//! Faker's classes: each the documentation names answers, the same seed the same value.

use super::*;

#[test]
fn every_class_the_documentation_names_answers() {
    let mut next = seeded(7);
    for (class, method) in [
        ("address", "fullAddress"),
        ("beer", "name"),
        ("bool", "bool"),
        ("business", "creditCardNumber"),
        ("ChuckNorris", "fact"),
        ("code", "isbn13"),
        ("color", "name"),
        ("commerce", "productName"),
        ("company", "name"),
        ("crypto", "sha256"),
        ("educator", "university"),
        ("finance", "iban"),
        ("hacker", "noun"),
        ("idNumber", "valid"),
        ("internet", "emailAddress"),
        ("lorem", "sentence"),
        ("name", "fullName"),
        ("number", "randomNumber"),
        ("phoneNumber", "cellPhone"),
        ("shakespeare", "hamletQuote"),
        ("superhero", "name"),
        ("team", "name"),
        ("university", "name"),
    ] {
        let value = fake(class, method, &mut next);
        assert!(value.as_deref().is_some_and(|v| !v.is_empty()), "{class}.{method}");
    }
    assert_eq!(fake("name", "nope", &mut next), None);
}

#[test]
fn the_same_seed_gives_the_same_value_and_cards_pass_luhn() {
    let a = fake("name", "fullName", &mut seeded(12));
    let b = fake("name", "fullName", &mut seeded(12));
    assert_eq!(a, b);
    let number = fake("business", "creditCardNumber", &mut seeded(3)).unwrap();
    let sum: u32 = number
        .chars()
        .rev()
        .enumerate()
        .map(|(i, c)| {
            let d = c.to_digit(10).unwrap();
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    assert_eq!(sum % 10, 0, "{number}");
}
