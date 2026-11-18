//! Support for `.editorconfig` (https://editorconfig.org).
//!
//! For a file, all `.editorconfig` files from its folder upwards are read, up to the one
//! with `root = true`. Closer files and later sections within a file take
//! precedence.

use std::io;
use std::path::{Path, PathBuf};

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Space,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndOfLine {
    Lf,
    CrLf,
    Cr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    Utf8,
    Utf8Bom,
    Latin1,
    Utf16Le,
    Utf16Be,
}

/// The properties that apply to a file; `None` = unspecified.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorConfig {
    pub indent_style: Option<IndentStyle>,
    pub indent_size: Option<u8>,
    pub tab_width: Option<u8>,
    pub end_of_line: Option<EndOfLine>,
    pub charset: Option<Charset>,
    pub trim_trailing_whitespace: Option<bool>,
    pub insert_final_newline: Option<bool>,
    pub max_line_length: Option<u32>,
}

impl EditorConfig {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Applies a property (`key`, `value` already lowercase). Unknown values
    /// or `unset` leave the property unspecified.
    fn set(&mut self, key: &str, value: &str) {
        let flag = |v: &str| match v {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        match key {
            "indent_style" => {
                self.indent_style = match value {
                    "space" => Some(IndentStyle::Space),
                    "tab" => Some(IndentStyle::Tab),
                    _ => None,
                }
            }
            // `indent_size = tab` means "as much as tab_width".
            "indent_size" => {
                self.indent_size = value
                    .parse()
                    .ok()
                    .filter(|&n: &u8| n > 0)
                    .or(if value == "tab" { self.tab_width } else { None })
            }
            "tab_width" => self.tab_width = value.parse().ok().filter(|&n: &u8| n > 0),
            "end_of_line" => {
                self.end_of_line = match value {
                    "lf" => Some(EndOfLine::Lf),
                    "crlf" => Some(EndOfLine::CrLf),
                    "cr" => Some(EndOfLine::Cr),
                    _ => None,
                }
            }
            "charset" => {
                self.charset = match value {
                    "utf-8" => Some(Charset::Utf8),
                    "utf-8-bom" => Some(Charset::Utf8Bom),
                    "latin1" => Some(Charset::Latin1),
                    "utf-16le" => Some(Charset::Utf16Le),
                    "utf-16be" => Some(Charset::Utf16Be),
                    _ => None,
                }
            }
            "trim_trailing_whitespace" => self.trim_trailing_whitespace = flag(value),
            "insert_final_newline" => self.insert_final_newline = flag(value),
            "max_line_length" => self.max_line_length = value.parse().ok(),
            _ => {}
        }
    }

    /// Fills in the defaults from the spec (e.g. `tab_width` = `indent_size`).
    fn finish(mut self) -> Self {
        if self.indent_style == Some(IndentStyle::Tab) && self.indent_size.is_none() {
            self.indent_size = self.tab_width;
        }
        if self.tab_width.is_none() {
            self.tab_width = self.indent_size;
        }
        self
    }
}

/// A parsed `.editorconfig` file.
#[derive(Debug, Clone)]
pub struct EditorConfigFile {
    dir: PathBuf,
    root: bool,
    sections: Vec<(Regex, Vec<(String, String)>)>,
}

impl EditorConfigFile {
    /// Parses the contents of the file located in `dir`. Sections with an invalid glob are
    /// ignored.
    pub fn parse(dir: &Path, source: &str) -> Self {
        let mut file = Self { dir: dir.to_path_buf(), root: false, sections: Vec::new() };
        let mut current: Option<(Regex, Vec<(String, String)>)> = None;
        for raw in source.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(glob) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                file.sections.extend(current.take());
                current = glob_to_regex(glob).map(|r| (r, Vec::new()));
                continue;
            }
            let Some((key, value)) = line.split_once(['=', ':']) else { continue };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_ascii_lowercase();
            match &mut current {
                Some((_, props)) => props.push((key, value)),
                None if key == "root" => file.root = value == "true",
                None => {}
            }
        }
        file.sections.extend(current);
        file
    }

    pub fn is_root(&self) -> bool {
        self.root
    }

    fn apply(&self, path: &Path, config: &mut EditorConfig) {
        let Ok(relative) = path.strip_prefix(&self.dir) else { return };
        let relative = relative.to_string_lossy().replace('\\', "/");
        for (glob, props) in &self.sections {
            if glob.is_match(&relative) {
                for (k, v) in props {
                    config.set(k, v);
                }
            }
        }
    }
}

/// The port for reading `.editorconfig` files (implemented in infrastructure).
pub trait EditorConfigSource {
    /// The contents of `dir/.editorconfig`, if it exists.
    fn read_editorconfig(&self, dir: &Path) -> io::Result<Option<String>>;
}

/// The properties for the file at `path` (absolute path).
pub fn resolve(path: &Path, source: &dyn EditorConfigSource) -> EditorConfig {
    let mut files = Vec::new();
    let mut dir = path.parent();
    while let Some(d) = dir {
        if let Ok(Some(text)) = source.read_editorconfig(d) {
            let file = EditorConfigFile::parse(d, &text);
            let root = file.is_root();
            files.push(file);
            if root {
                break;
            }
        }
        dir = d.parent();
    }
    let mut config = EditorConfig::default();
    // From the farthest to the closest: the closer one overrides.
    for file in files.iter().rev() {
        file.apply(path, &mut config);
    }
    config.finish()
}

/// Converts an EditorConfig glob into a regular expression on the relative path.
fn glob_to_regex(glob: &str) -> Option<Regex> {
    let glob = glob.trim();
    // Without `/`, the glob matches the file name in any folder.
    let (glob, anywhere) = match glob.strip_prefix('/') {
        Some(rest) => (rest, false),
        None => (glob, !glob.contains('/')),
    };
    let mut re = String::from(if anywhere { "^(?:.*/)?" } else { "^" });
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    let mut braces = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                re.push_str(".*");
                i += 1;
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '[' => {
                let end = chars[i..].iter().position(|&c| c == ']')? + i;
                let class: String = chars[i + 1..end].iter().collect();
                let class =
                    class.strip_prefix('!').map_or(class.clone(), |rest| format!("^{rest}"));
                re.push('[');
                re.push_str(&class.replace('/', ""));
                re.push(']');
                i = end;
            }
            '{' => {
                let end = chars[i..].iter().position(|&c| c == '}').map(|p| p + i);
                let inner: String =
                    end.map(|e| chars[i + 1..e].iter().collect()).unwrap_or_default();
                // `{1..10}`: numeric range.
                if let Some((a, b)) = inner.split_once("..")
                    && let (Ok(a), Ok(b)) = (a.parse::<i64>(), b.parse::<i64>())
                {
                    let alternatives: Vec<String> =
                        (a.min(b)..=a.max(b)).map(|n| n.to_string()).collect();
                    re.push_str(&format!("(?:{})", alternatives.join("|")));
                    i = end.unwrap_or(i);
                } else if inner.contains(',') {
                    re.push_str("(?:");
                    braces += 1;
                } else {
                    re.push_str(&regex::escape(&format!("{{{inner}}}")));
                    i = end.unwrap_or(i);
                }
            }
            ',' if braces > 0 => re.push('|'),
            '}' if braces > 0 => {
                re.push(')');
                braces -= 1;
            }
            // The spaces in `{motd, pre-commit}` aren't part of the names.
            ' ' if braces > 0 => {}
            '\\' if i + 1 < chars.len() => {
                re.push_str(&regex::escape(&chars[i + 1].to_string()));
                i += 1;
            }
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).ok()
}

#[cfg(test)]
#[path = "../tests/unit/editorconfig.rs"]
mod tests;
