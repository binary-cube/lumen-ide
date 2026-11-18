use super::*;

const SYMFONY: &str = include_str!("../data/symfony.env");

#[test]
fn hides_the_secrets_of_a_symfony_file() {
    let hidden: Vec<&str> = secrets(SYMFONY).into_iter().map(|r| &SYMFONY[r]).collect();
    assert_eq!(
        hidden,
        [
            "0b1fc5d3c8f1e4c7a2d9e6b5a4f3e2d1",
            "!ChangeMe!",
            "s3cret",
            "%kernel.project_dir%/config/jwt/private.pem",
            "%kernel.project_dir%/config/jwt/public.pem",
            "5c4f0e8b",
        ]
    );
}

#[test]
fn nothing_is_hidden_while_the_setting_is_off() {
    settings::set_mask_secrets(false);
    assert!(Secrets.concealed("APP_SECRET=x\n").is_empty());
    let before = Secrets.generation();
    settings::set_mask_secrets(true);
    assert_ne!(Secrets.generation(), before);
    assert_eq!(Secrets.concealed("APP_SECRET=x\n"), vec![11..12]);
    settings::set_mask_secrets(false);
}
