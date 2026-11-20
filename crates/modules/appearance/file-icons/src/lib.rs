//! File icons: rules by exact name, glob and extension, plus the SVG icons
//! they need. The color comes from the theme (the `icon.<color>` token).
//!
//! Files that have a language (PHP, JS…) get the language icon; the rules here
//! cover the rest (configs, images, lock files) and special cases (`composer.json`).

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{FileIconRule, FileMatcher, FolderColorRule, IconSvg};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("file-icons"),
    name: "File icons",
    description: "Icons in the tree and tabs by file name or extension",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file", ..extensions::ModuleInfo::BUILT_IN },
};

const ICONS: &[IconSvg] = &[
    IconSvg { name: "key", svg: include_str!("../svg/key.svg") },
    IconSvg { name: "git", svg: include_str!("../svg/git.svg") },
    IconSvg { name: "lock", svg: include_str!("../svg/lock.svg") },
    IconSvg { name: "image", svg: include_str!("../svg/image.svg") },
    IconSvg { name: "markdown", svg: include_str!("../svg/markdown.svg") },
    IconSvg { name: "docker", svg: include_str!("../svg/docker.svg") },
    IconSvg { name: "database", svg: include_str!("../svg/database.svg") },
    IconSvg { name: "archive", svg: include_str!("../svg/archive.svg") },
    IconSvg { name: "terminal", svg: include_str!("../svg/terminal.svg") },
    IconSvg { name: "settings", svg: include_str!("../svg/settings.svg") },
    IconSvg { name: "package", svg: include_str!("../svg/package.svg") },
    IconSvg { name: "file-lines", svg: include_str!("../svg/file-lines.svg") },
    IconSvg { name: "layers", svg: include_str!("../svg/layers.svg") },
];

/// (match, icon, color)
const RULES: &[(FileMatcher, &str, &str)] = {
    use FileMatcher::{Extension as Ext, FileName as Name, Glob};
    &[
        (Name(".env"), "key", "env"),
        (Glob(".env.*"), "key", "env"),
        (Glob("*.pem"), "key", "env"),
        (Ext("key"), "key", "env"),
        (Name(".gitignore"), "git", "git"),
        (Name(".gitattributes"), "git", "git"),
        (Name(".gitmodules"), "git", "git"),
        (Name(".gitkeep"), "git", "git"),
        (Name("composer.json"), "package", "composer"),
        (Name("package.json"), "package", "npm"),
        (Name("composer.lock"), "lock", "lock"),
        (Name("package-lock.json"), "lock", "lock"),
        (Name("yarn.lock"), "lock", "lock"),
        (Name("pnpm-lock.yaml"), "lock", "lock"),
        (Name("cargo.lock"), "lock", "lock"),
        (Ext("lock"), "lock", "lock"),
        // Their color key is their language's (`dockerfile`): a file icon set finds their
        // icon by it (a `Dockerfile` has no extension to go by).
        (Name("dockerfile"), "docker", "dockerfile"),
        (Glob("dockerfile.*"), "docker", "dockerfile"),
        (Glob("docker-compose*.yml"), "docker", "docker"),
        (Glob("docker-compose*.yaml"), "docker", "docker"),
        (Glob("compose*.yaml"), "docker", "docker"),
        (Name(".dockerignore"), "docker", "docker"),
        (Name(".editorconfig"), "settings", "config"),
        (Glob(".prettierrc*"), "settings", "config"),
        (Glob(".eslintrc*"), "settings", "config"),
        (Glob("phpunit.xml*"), "settings", "config"),
        (Glob("phpstan.neon*"), "settings", "config"),
        (Glob("*.config.js"), "settings", "config"),
        (Glob("*.config.ts"), "settings", "config"),
        (Glob("*.config.mjs"), "settings", "config"),
        (Ext("ini"), "settings", "config"),
        (Ext("toml"), "settings", "config"),
        (Ext("conf"), "settings", "config"),
        (Ext("neon"), "settings", "config"),
        (Ext("xml"), "file-code", "xml"),
        (Ext("dist"), "settings", "config"),
        (Ext("md"), "markdown", "markdown"),
        (Ext("markdown"), "markdown", "markdown"),
        (Ext("txt"), "file-lines", "text"),
        (Ext("log"), "file-lines", "text"),
        (Ext("csv"), "file-lines", "text"),
        (Ext("vue"), "layers", "vue"),
        (Ext("jsx"), "file-code", "react"),
        (Ext("tsx"), "file-code", "react"),
        (Ext("ts"), "file-code", "typescript"),
        (Ext("sql"), "database", "database"),
        (Ext("sqlite"), "database", "database"),
        (Ext("db"), "database", "database"),
        (Ext("sh"), "terminal", "shell"),
        (Ext("bash"), "terminal", "shell"),
        (Ext("zsh"), "terminal", "shell"),
        (Ext("bat"), "terminal", "shell"),
        (Ext("ps1"), "terminal", "shell"),
        (Name("makefile"), "terminal", "shell"),
        (Ext("zip"), "archive", "archive"),
        (Ext("gz"), "archive", "archive"),
        (Ext("tgz"), "archive", "archive"),
        (Ext("tar"), "archive", "archive"),
        (Ext("rar"), "archive", "archive"),
        (Ext("7z"), "archive", "archive"),
        (Ext("phar"), "archive", "archive"),
        (Ext("png"), "image", "image"),
        (Ext("jpg"), "image", "image"),
        (Ext("jpeg"), "image", "image"),
        (Ext("gif"), "image", "image"),
        (Ext("webp"), "image", "image"),
        (Ext("avif"), "image", "image"),
        (Ext("ico"), "image", "image"),
        (Ext("bmp"), "image", "image"),
        (Ext("svg"), "image", "svg"),
    ]
};

/// Folders by what they hold: (names, color key). Sources, tests and libraries by role (the
/// palette's colors unless the theme has `icon.source` / `icon.test` / `icon.library`); the
/// rest with the files' colors of their kind.
const FOLDERS: &[(&[&str], &str)] = &[
    (&["src", "source", "sources", "app", "lib"], "source"),
    (&["tests", "test", "spec", "specs", "__tests__"], "test"),
    (&["vendor", "node_modules", "bower_components"], "library"),
    (&["config", "configs", "conf", ".config"], "config"),
    (&["docs", "doc", "documentation"], "markdown"),
    (&[".git", ".github", ".gitlab"], "git"),
    (&["docker", ".docker"], "docker"),
    (&["database", "migrations", "seeders", "db"], "database"),
    (&["public", "assets", "resources", "static", "www", "web"], "image"),
    (&["views", "templates"], "html"),
];

#[derive(Debug, Default)]
pub struct FileIconsModule;

impl Module for FileIconsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        for icon in ICONS {
            ctx.contribute(*icon);
        }
        for (matcher, icon, color) in RULES {
            ctx.contribute(FileIconRule::new(matcher.clone(), icon, color));
        }
        for (names, color) in FOLDERS {
            for name in *names {
                ctx.contribute(FolderColorRule { name, color });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
