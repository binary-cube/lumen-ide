use super::*;

struct JsonSniffer;
impl ContentSniffer for JsonSniffer {
    fn language(&self) -> LanguageId {
        LanguageId::new_static("json")
    }
    fn sniff(&self, text: &str) -> Option<Confidence> {
        text.trim_start().starts_with('{').then_some(Confidence(80))
    }
}

fn registry() -> LanguageRegistry {
    let mut r = LanguageRegistry::default();
    r.register(
        LanguageDescriptor::new("php", "PHP").extensions(&["php", "phtml"]).shebangs(&["php"]),
    );
    r.register(LanguageDescriptor::new("blade", "Blade").globs(&["*.blade.php"]));
    r.register(
        LanguageDescriptor::new("json", "JSON")
            .extensions(&["json"])
            .file_names(&["composer.lock"]),
    );
    r.register(LanguageDescriptor::new("shell", "Shell").shebangs(&["bash", "sh"]));
    r.register_sniffer(Arc::new(JsonSniffer));
    r
}

fn detect(path: &str, head: &str) -> String {
    registry().detect(Path::new(path), head).to_string()
}

#[test]
fn an_open_ended_glob_gives_way_to_another_languages_extension() {
    let mut r = registry();
    r.register(
        LanguageDescriptor::new("dotenv", ".env")
            .file_names(&[".env"])
            .globs(&[".env.*"])
            .extensions(&["env"]),
    );
    let detect = |path: &str| r.detect(Path::new(path), "").to_string();
    for name in [".env", ".env.local", ".env.prod.local", ".env.example", "docker.env"] {
        assert_eq!(detect(name), "dotenv", "{name}");
    }
    // Symfony's compiled `.env.local.php`, `.env.json`: their own languages.
    assert_eq!(detect(".env.local.php"), "php");
    assert_eq!(detect(".env.json"), "json");
}

#[test]
fn detection_order() {
    assert_eq!(detect("src/Kernel.php", ""), "php");
    assert_eq!(detect("views/home.blade.php", ""), "blade");
    assert_eq!(detect("composer.lock", ""), "json");
    assert_eq!(detect("Index.PHP", ""), "php");
    assert_eq!(detect("bin/console", "#!/usr/bin/env php\n<?php"), "php");
    assert_eq!(detect("deploy", "#!/bin/bash -e\n"), "shell");
    assert_eq!(detect("data", "  {\"a\": 1}"), "json");
    assert_eq!(detect("notes", "hello"), "plaintext");
}

#[test]
fn file_icons_by_name_glob_extension_and_language() {
    let mut r = registry();
    r.register_icon_rule(FileIconRule::new(FileMatcher::FileName(".env"), "key", "env"));
    r.register_icon_rule(FileIconRule::new(FileMatcher::Glob(".env.*"), "key", "env"));
    r.register_icon_rule(FileIconRule::new(FileMatcher::Extension("png"), "image", "image"));
    r.register_icon_rule(FileIconRule::new(FileMatcher::FileName("composer.lock"), "lock", "lock"));
    assert_eq!(r.icon_for(".env"), ("key", "env".into()));
    assert_eq!(r.icon_for(".env.local"), ("key", "env".into()));
    assert_eq!(r.icon_for("Logo.PNG"), ("image", "image".into()));
    assert_eq!(r.icon_for("composer.lock"), ("lock", "lock".into()));
    assert_eq!(r.icon_for("Kernel.php"), ("file-code", "php".into()));
    assert_eq!(r.icon_for("notes"), ("file", "file".into()));
}

#[test]
fn folders_take_their_color_by_name_the_last_rule_winning() {
    let mut r = registry();
    r.register_folder_rule(FolderColorRule { name: "src", color: "source" });
    r.register_folder_rule(FolderColorRule { name: "vendor", color: "library" });
    r.register_folder_rule(FolderColorRule { name: "vendor", color: "config" });
    assert_eq!(r.folder_color("Src"), Some("source"));
    assert_eq!(r.folder_color("vendor"), Some("config"));
    assert_eq!(r.folder_color("srcs"), None);
}

#[test]
fn later_registration_replaces_language() {
    let mut r = registry();
    r.register(LanguageDescriptor::new("php", "PHP 8").extensions(&["php"]).line_comment("//"));
    assert_eq!(r.get(&LanguageId::new_static("php")).map(|l| l.name), Some("PHP 8"));
}

#[test]
fn shebang_parsing() {
    assert_eq!(shebang("#!/usr/bin/env -S php8.2 -d x\n"), Some("php"));
    assert_eq!(shebang("#!/usr/local/bin/node\n"), Some("node"));
    assert_eq!(shebang("<?php"), None);
}
