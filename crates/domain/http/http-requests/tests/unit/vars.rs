//! `{{name}}` resolved from the places in order, values holding `{{…}}` followed, the
//! dynamic ones made by a test's clock and dice, the names nothing has said together.

use super::*;
use std::collections::BTreeMap;

struct Fixed(u64);

impl Dynamic for Fixed {
    fn now(&self) -> u64 {
        1_762_862_400
    }
    fn uuid(&mut self) -> String {
        "00000000-0000-4000-8000-000000000000".into()
    }
    fn random(&mut self) -> u64 {
        self.0 += 7;
        self.0
    }
}

fn map(pairs: &[(&str, &str, bool)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(n, v, s)| ((*n).into(), Value { text: (*v).into(), secret: *s })).collect()
}

#[test]
fn names_come_from_the_first_place_that_has_them() {
    let request = map(&[("id", "42", false)]);
    let env =
        map(&[("host", "api.example.com", false), ("id", "1", false), ("token", "t0k", true)]);
    let file =
        [crate::FileVariable { name: "base".into(), value: "https://{{host}}/v1".into(), line: 0 }];
    let file = FileVariables(&file);
    let layers = Layers::new().then(&request).then(&file).then(&env);
    let resolved = resolve("{{base}}/users/{{ id }}?t={{token}}", &layers, &mut Fixed(0)).unwrap();
    assert_eq!(resolved.text, "https://api.example.com/v1/users/42?t=t0k");
    assert!(resolved.secret, "a secret went in");
    assert_eq!(references("a {{x}} b {{ y.z }}"), [(2, "x"), (10, "y.z")]);
}

#[test]
fn names_nothing_has_are_said_not_sent_empty() {
    let empty = map(&[]);
    let err = resolve("{{a}}/{{b}}/{{a}}", &empty, &mut Fixed(0)).unwrap_err();
    assert_eq!(err.0, ["a", "b"]);
    // A loop stops.
    let looped = map(&[("a", "{{a}}", false)]);
    assert!(resolve("{{a}}", &looped, &mut Fixed(0)).is_ok());
}

#[test]
fn the_dynamic_ones() {
    let empty = map(&[]);
    let one = |text: &str| resolve(text, &empty, &mut Fixed(0)).unwrap().text;
    assert_eq!(one("{{$uuid}}"), "00000000-0000-4000-8000-000000000000");
    assert_eq!(one("{{$random.uuid}}"), one("{{$uuid}}"));
    assert_eq!(one("{{$timestamp}}"), "1762862400");
    assert_eq!(one("{{$isoTimestamp}}"), "2025-11-11T12:00:00Z");
    assert_eq!(one("{{$randomInt}}"), "7");
    assert_eq!(one("{{$random.integer(10, 20)}}"), "17");
    let wide: i128 =
        one("{{$random.integer(-9223372036854775808, 9223372036854775807)}}").parse().unwrap();
    assert!(wide >= i128::from(i64::MIN) && wide <= i128::from(i64::MAX), "no overflow");
    // Not a dynamic one: a JSONPath over the environment.
    let env = map(&[("$.clients..id", "[1,2]", false)]);
    assert_eq!(resolve("{{$.clients..id}}", &env, &mut Fixed(0)).unwrap().text, "[1,2]");
    assert_eq!(one("{{$random.alphanumeric(3)}}").len(), 3);
    assert!(resolve("{{$nope}}", &empty, &mut Fixed(0)).is_err());
    assert_eq!(one("{{$random.hexadecimal(4)}}").len(), 4);
    assert!(one("{{$random.hexadecimal(6)}}").chars().all(|c| c.is_ascii_hexdigit()));
    assert!(one("{{$random.alphabetic(5)}}").chars().all(|c| c.is_ascii_alphabetic()));
    assert!(one("{{$random.email}}").ends_with("@example.com"));
    let float: f64 = one("{{$random.float(10.5, 20.3)}}").parse().unwrap();
    assert!((10.5..20.3).contains(&float));
    assert!(resolve("{{$env.HOME}}", &empty, &mut Fixed(0)).is_err(), "no environment told");
    assert_eq!(iso_timestamp(0), "1970-01-01T00:00:00Z");
    assert_eq!(iso_timestamp(951_782_400), "2000-02-29T00:00:00Z");
}

#[test]
fn a_request_is_resolved_whole_or_says_every_name_missing() {
    let file = crate::parse(
        "POST https://{{host}}/x\nAuthorization: Bearer {{token}}\n\n{\"id\": \"{{id}}\"}\n< ./{{name}}.json\n",
    );
    let env = map(&[
        ("host", "h", false),
        ("token", "s", true),
        ("id", "1", false),
        ("name", "n", false),
    ]);
    let resolved = resolve_request(&file.requests[0], &env, &mut Fixed(0)).unwrap();
    assert_eq!(resolved.url, "https://h/x");
    assert_eq!(resolved.headers[0].value, "Bearer s");
    assert_eq!(
        resolved.body[0],
        crate::BodyPart::Text("{\"id\": \"1\"}\n".into()),
        "its line's end before the file"
    );
    assert_eq!(resolved.body[1], crate::BodyPart::File("./n.json".into()));
    assert!(resolved.secret);
    assert_eq!(resolved.secrets, ["s"], "the secret's value as it went in");
    let long = ResolvedRequest { secrets: vec!["t0ken".into(), "ab".into()], ..resolved };
    assert_eq!(long.masked("?k=t0ken&ab=1"), "?k=••••••&ab=1", "too short to hide: kept");
    let partial = map(&[("host", "h", false)]);
    let err = resolve_request(&file.requests[0], &partial, &mut Fixed(0)).unwrap_err();
    assert_eq!(err.0, ["token", "id", "name"]);
}

#[test]
fn faker_and_folders() {
    let empty = map(&[]);
    let one = |text: &str| resolve(text, &empty, &mut Fixed(0)).unwrap().text;
    assert!(one("{{$random.name.fullName}}").contains(' '));
    assert!(one("{{$random.internet.emailAddress}}").contains('@'));
    let a = one("{{$random.locale.name.fullName(\"fr\", 12)}}");
    assert_eq!(a, one("{{$random.locale.name.fullName(\"fr\", 12)}}"), "the same seed");
    assert!(resolve("{{$random.name.nope}}", &empty, &mut Fixed(0)).is_err());
    assert!(resolve("{{$projectRoot}}", &empty, &mut Fixed(0)).is_err(), "not told");
}
