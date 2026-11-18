use super::*;
use std::collections::HashMap;

const SITE: &str = "# a comment\nroot = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\nindent_style = space\nindent_size = 4\ntrim_trailing_whitespace = true\ninsert_final_newline = true\nmax_line_length = 120\n\n[*.md]\ntrim_trailing_whitespace = false\ninsert_final_newline = false\n\n[Makefile]\nindent_style = tab\n\n[{motd, pre-commit}]\ntrim_trailing_whitespace = false\ninsert_final_newline = false\n";

struct Files(HashMap<PathBuf, &'static str>);

impl EditorConfigSource for Files {
    fn read_editorconfig(&self, dir: &Path) -> io::Result<Option<String>> {
        Ok(self.0.get(dir).map(|s| (*s).to_owned()))
    }
}

fn project() -> Files {
    Files(HashMap::from([
        (PathBuf::from("/work"), "root = true\n[*]\nindent_size = 2\n"),
        (PathBuf::from("/work/site"), SITE),
        (PathBuf::from("/work/site/legacy"), "[*.php]\nindent_style = tab\n"),
    ]))
}

#[test]
fn real_world_file() {
    let c = resolve(Path::new("/work/site/src/Kernel.php"), &project());
    assert_eq!(c.indent_style, Some(IndentStyle::Space));
    assert_eq!(c.indent_size, Some(4));
    assert_eq!(c.tab_width, Some(4));
    assert_eq!(c.end_of_line, Some(EndOfLine::Lf));
    assert_eq!(c.charset, Some(Charset::Utf8));
    assert_eq!(c.trim_trailing_whitespace, Some(true));
    assert_eq!(c.insert_final_newline, Some(true));
    assert_eq!(c.max_line_length, Some(120));
}

#[test]
fn later_sections_override_and_root_stops_search() {
    let md = resolve(Path::new("/work/site/README.md"), &project());
    assert_eq!(md.trim_trailing_whitespace, Some(false));
    assert_eq!(md.insert_final_newline, Some(false));
    let make = resolve(Path::new("/work/site/Makefile"), &project());
    assert_eq!(make.indent_style, Some(IndentStyle::Tab));
    let hook = resolve(Path::new("/work/site/bin/pre-commit"), &project());
    assert_eq!(hook.insert_final_newline, Some(false));
    // `site/.editorconfig` has root = true, so `/work/.editorconfig` (indent 2) is ignored.
    assert_eq!(resolve(Path::new("/work/site/a.js"), &project()).indent_size, Some(4));
    assert_eq!(resolve(Path::new("/work/other/a.js"), &project()).indent_size, Some(2));
}

#[test]
fn closer_file_wins() {
    let c = resolve(Path::new("/work/site/legacy/old.php"), &project());
    assert_eq!(c.indent_style, Some(IndentStyle::Tab));
    assert_eq!(c.max_line_length, Some(120));
}

#[test]
fn glob_features() {
    let m = |g: &str, p: &str| glob_to_regex(g).unwrap().is_match(p);
    assert!(m("*.php", "src/A.php"));
    assert!(!m("/*.php", "src/A.php"));
    assert!(m("src/**/*.js", "src/a/b/c.js"));
    assert!(m("{*.json,*.yml}", "config/app.yml"));
    assert!(m("file{1..3}.txt", "file2.txt"));
    assert!(!m("file{1..3}.txt", "file4.txt"));
    assert!(m("[ab].txt", "a.txt"));
    assert!(!m("[!ab].txt", "a.txt"));
    assert!(m("?.md", "x.md"));
}

#[test]
fn no_files_means_empty_config() {
    assert!(resolve(Path::new("/elsewhere/x.php"), &project()).is_empty());
}

/// A size of 0 isn't a size: left unset (it would divide by zero).
#[test]
fn a_zero_size_is_ignored() {
    let files = Files(HashMap::from([(
        PathBuf::from("/z"),
        "root = true\n[*]\nindent_style = space\nindent_size = 0\ntab_width = 0\n",
    )]));
    let c = resolve(Path::new("/z/a.php"), &files);
    assert_eq!((c.indent_size, c.tab_width), (None, None));
}
