use super::*;

/// The names `accesses` find in `text`.
fn names<'a>(text: &'a str, accesses: &[EnvAccess]) -> Vec<&'a str> {
    reads(text, accesses).into_iter().map(|r| &text[r]).collect()
}

#[test]
fn php_calls_and_superglobals() {
    let accesses =
        [EnvAccess::call("getenv"), EnvAccess::index("$_ENV"), EnvAccess::index("$_SERVER")];
    let text = r#"<?php
$a = getenv('APP_ENV');
$b = \getenv( "DB_HOST" ) ?: 'x';
$c = $_ENV['MAILER_DSN'] ?? $_SERVER["APP_SECRET"];
$d = mygetenv('NOT'); $e = $this->getenv('NOT'); $f = getenv($dynamic);
$g = 'x' . getenv('JOINED');
"#;
    assert_eq!(
        names(text, &accesses),
        ["APP_ENV", "DB_HOST", "MAILER_DSN", "APP_SECRET", "JOINED"]
    );
}

#[test]
fn laravel_and_symfony_shapes() {
    let laravel = [EnvAccess::call("env"), EnvAccess::call("Env::get")];
    let text = "'name' => env('APP_NAME', 'Laravel'), 'k' => Env::get('APP_KEY'), $app->env('NOT')";
    assert_eq!(names(text, &laravel), ["APP_NAME", "APP_KEY"]);
    let symfony = [EnvAccess::placeholder("%env(", &[")%"]), EnvAccess::named("env")];
    let yaml = "secret: '%env(APP_SECRET)%'\nport: '%env(int:DB_PORT)%'\nx: '%env(default:fallback:resolve:DSN)%'\nbad: '%env(int:)%'\n";
    assert_eq!(names(yaml, &symfony), ["APP_SECRET", "DB_PORT", "DSN"]);
    let php = "#[Autowire(env: 'SLACK_URL')] string $url, #[Autowire(env:\"X_KEY\")] $k";
    assert_eq!(names(php, &symfony), ["SLACK_URL", "X_KEY"]);
}

#[test]
fn javascript_members_and_indexes() {
    let js = [
        EnvAccess::member("process.env"),
        EnvAccess::index("process.env"),
        EnvAccess::member("import.meta.env"),
    ];
    let text = "const a = process.env.API_URL, b = process.env['TOKEN'], c = import.meta.env.VITE_APP_NAME;";
    assert_eq!(names(text, &js), ["API_URL", "TOKEN", "VITE_APP_NAME"]);
}

#[test]
fn compose_and_shell_interpolations() {
    let shell = [EnvAccess::INTERPOLATION, EnvAccess::VARIABLE];
    let text = "image: app:${TAG:-latest}\nports: [\"${PORT}:80\"]\ncommand: echo $HOME ${BAD SPACE} $$ESC\n";
    assert_eq!(names(text, &shell), ["TAG", "PORT", "HOME"], "`$$` is a dollar");
}

#[test]
fn the_read_at_the_cursor() {
    let accesses = [EnvAccess::call("getenv")];
    let text = "a();\n$x = getenv('APP_ENV');\n";
    let at = text.find("APP_ENV").unwrap();
    assert_eq!(read_at(text, &accesses, at + 3).map(|r| &text[r]), Some("APP_ENV"));
    assert_eq!(
        read_at(text, &accesses, at + 7).map(|r| &text[r]),
        Some("APP_ENV"),
        "right after it"
    );
    assert_eq!(read_at(text, &accesses, 1), None);
}

#[test]
fn a_files_reads_come_from_the_contributions_covering_it() {
    const PHP: EnvReads = EnvReads::new(&["php"], &[EnvAccess::call("getenv")]);
    const SHELL: EnvReads = EnvReads::new(&["bash"], &[EnvAccess::VARIABLE]).unflagged();
    const NODE: EnvReads = EnvReads::new(&["javascript"], &[EnvAccess::member("process.env")])
        .providing(&["NODE_ENV"]);
    let all = [PHP, SHELL, NODE];
    let php = FileReads::of(&all, "php", "index.php");
    assert_eq!((php.accesses.len(), php.flagged.len()), (1, 1));
    let shell = FileReads::of(&all, "bash", "deploy.sh");
    assert_eq!((shell.accesses.len(), shell.flagged.len()), (1, 0));
    assert_eq!(FileReads::of(&all, "javascript", "a.js").provided, ["NODE_ENV"]);
    assert!(FileReads::of(&all, "css", "a.css").is_empty());
}
