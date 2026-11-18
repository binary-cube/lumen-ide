use super::*;

#[test]
fn sections_are_registered_once() {
    let fenced = EnvSections::Fenced { open: "###> ", close: "###< ", trail: " ###" };
    register_sections(fenced);
    register_sections(fenced);
    register_sections(EnvSections::KeyPrefix);
    let all = sections();
    assert_eq!(all.iter().filter(|s| **s == fenced).count(), 1);
    assert!(all.contains(&EnvSections::KeyPrefix));
}

const SYMFONY: EnvFileOrder = EnvFileOrder {
    name: "Symfony",
    markers: &["symfony.lock"],
    files: &[".env", ".env.local", ".env.{env}", ".env.{env}.local"],
};

#[test]
fn a_files_rank_in_its_frameworks_order() {
    let rank = |name: &str| SYMFONY.rank(name);
    assert_eq!(rank(".env"), Some(0));
    assert_eq!(rank(".env.local"), Some(1), "named, before the environment's pattern");
    assert_eq!(rank(".env.prod"), Some(2));
    assert_eq!(rank(".env.prod.local"), Some(3));
    assert_eq!(rank(".ENV.Test"), Some(2));
    assert_eq!(rank("docker.env"), None);
    assert_eq!(rank(".env."), None);
}

#[test]
fn reads_cover_their_languages_and_files() {
    const COMPOSE: EnvReads = EnvReads::new(&["yaml"], &[EnvAccess::INTERPOLATION])
        .in_files(&["compose*.yaml", "docker-compose*.yml"])
        .unflagged();
    assert!(COMPOSE.covers("yaml", "compose.yaml"));
    assert!(COMPOSE.covers("yaml", "Docker-Compose.prod.yml"));
    assert!(!COMPOSE.covers("yaml", "services.yaml"));
    assert!(!COMPOSE.covers("php", "compose.yaml"));
    const PHP: EnvReads = EnvReads::new(&["php"], &[EnvAccess::call("getenv")]);
    assert!(PHP.covers("php", "Kernel.php") && PHP.flag_undefined && !COMPOSE.flag_undefined);
}

#[test]
fn the_constructors_say_the_common_shapes() {
    let call = EnvAccess::call("getenv");
    assert_eq!(
        (call.prefix, call.opening, call.quoting, call.word),
        ("getenv", "(", Quoting::Quoted, true)
    );
    assert_eq!(EnvAccess::index("$_ENV").after, &["]"]);
    assert_eq!(EnvAccess::member("process.env").quoting, Quoting::Bare);
    let placeholder = EnvAccess::placeholder("%env(", &[")%"]);
    assert!(placeholder.processors && !placeholder.word);
}
