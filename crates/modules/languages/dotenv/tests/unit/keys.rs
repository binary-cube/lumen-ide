use super::*;
use code_intelligence::env::EnvAccess;

const PHP: EnvReads = EnvReads::new(&["php"], &[EnvAccess::call("getenv")]);

#[test]
fn the_key_at_the_cursor_in_a_env_file_and_in_code() {
    let env = "HOST=db\nURL=http://${HOST}/\n";
    let at = env.rfind("HOST").unwrap() + 1;
    assert_eq!(key_at(&[PHP], crate::ID, ".env", env, at).map(|r| &env[r]), Some("HOST"));
    assert_eq!(key_at(&[PHP], crate::ID, ".env", env, env.find("http").unwrap()), None);
    let php = "<?php $h = getenv('HOST');";
    let at = php.find("HOST").unwrap();
    assert_eq!(key_at(&[PHP], "php", "a.php", php, at).map(|r| &php[r]), Some("HOST"));
    assert_eq!(key_at(&[PHP], "css", "a.css", php, at), None, "no way of reading there");
}

#[test]
fn every_place_a_key_is_written() {
    let env = "HOST=db\nURL=http://$HOST/\n";
    let found: Vec<&str> =
        places(&[PHP], crate::ID, ".env", env).into_iter().map(|r| &env[r]).collect();
    assert_eq!(found, ["HOST", "URL", "HOST"]);
    let php = "getenv('A'); getenv('B');";
    assert_eq!(places(&[PHP], "php", "a.php", php).len(), 2);
}
