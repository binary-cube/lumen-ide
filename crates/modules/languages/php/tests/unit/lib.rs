use super::*;

/// Where Composer installs: `config.vendor-dir` as `composer.json` writes it (relative, its
/// `./` and slashes made plain); nothing when it doesn't say, says `vendor`, or names a
/// folder outside the project.
#[test]
fn composer_s_vendor_dir_is_read_from_its_file() {
    let with =
        |dir: &str| format!(r#"{{"name": "shop/app", "config": {{"vendor-dir": "{dir}"}}}}"#);
    assert_eq!(composer_vendor_dir(&with("lib/composer")), ["lib/composer"]);
    assert_eq!(composer_vendor_dir(&with("./deps/")), ["deps"]);
    assert_eq!(composer_vendor_dir(&with("deps\\\\php")), ["deps/php"]);
    assert!(composer_vendor_dir(&with("vendor")).is_empty(), "the fixed name already");
    assert!(composer_vendor_dir(&with("../shared/vendor")).is_empty(), "outside the project");
    assert!(composer_vendor_dir(&with("/opt/vendor")).is_empty());
    assert!(composer_vendor_dir(r#"{"name": "shop/app"}"#).is_empty(), "not said");
    assert!(composer_vendor_dir("{ not json").is_empty());
}
