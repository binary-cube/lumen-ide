use super::*;

#[test]
fn nested_keys_are_joined_and_sections_kept_apart() {
    let yaml = "# greetings\nuser:\n    greeting: 'Hi %name%'\n    farewell: Bye # soon\n'title.home': Home\nlist:\n    - a\n";
    let found: Vec<(String, Option<String>)> =
        keys(yaml, None).into_iter().map(|k| (k.name, k.value)).collect();
    assert_eq!(
        found,
        [
            ("user.greeting".to_owned(), Some("Hi %name%".to_owned())),
            ("user.farewell".to_owned(), Some("Bye".to_owned())),
            ("title.home".to_owned(), Some("Home".to_owned())),
        ]
    );
    let config = "parameters:\n    app.admin_email: 'a@b.c'\n    locale: en\nservices:\n    x: ~\n";
    let params: Vec<String> =
        keys(config, Some("parameters")).into_iter().map(|k| k.name).collect();
    assert_eq!(params, ["app.admin_email", "locale"]);
    let k = &keys(config, Some("parameters"))[0];
    assert_eq!(&config[k.offset..k.offset + 15], "app.admin_email");
}
