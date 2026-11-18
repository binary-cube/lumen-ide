use super::*;

#[test]
fn finds_the_view_directory() {
    assert_eq!(controller_id("FooBarController.php").as_deref(), Some("foo-bar"));
    assert_eq!(controller_id("Model.php"), None);
    assert_eq!(
        view_dir(Path::new("/app/controllers/SiteController.php")),
        Some(PathBuf::from("/app/views/site"))
    );
    assert_eq!(
        view_dir(Path::new("/app/views/site/index.php")),
        Some(PathBuf::from("/app/views/site"))
    );
    assert_eq!(view_dir(Path::new("/app/models/User.php")), None);
}
