//! Environment variables and the `.env` files defining them: what other modules tell the
//! `.env` module about them, without it knowing any framework or language. Each is a
//! contribution (data, no code), put here by the app once the modules are activated
//! (`register_*`) and read by the `.env` module (no module names another):
//!
//! - [`EnvSections`]: how a framework marks the sections of its `.env` files (Symfony
//!   Flex's `###> vendor/package ###` blocks, Laravel's key prefixes);
//! - [`EnvReads`]: how a language's code reads a variable (`getenv('X')`,
//!   `process.env.X`, `%env(int:X)%`, `${X:-default}`), each way an [`EnvAccess`];
//! - [`EnvFileOrder`]: the order a framework loads a project's `.env` files in (Symfony:
//!   `.env`, `.env.local`, `.env.{env}`, `.env.{env}.local`).

use std::sync::{Mutex, OnceLock};

/// How a framework marks the sections of its `.env` files. Recognized by the file's
/// content: the files are read without knowing their project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvSections {
    /// Blocks between two marker comments naming them: `open` + label + `trail` opens one,
    /// `close` + the same label + `trail` closes it — Symfony Flex's recipes:
    /// `###> symfony/mailer ###` … `###< symfony/mailer ###`. The label is the section's.
    Fenced { open: &'static str, close: &'static str, trail: &'static str },
    /// In a file without fenced blocks, the groups of keys with no comment above them (blank
    /// lines apart) named after their keys' common prefix (`APP_`, `DB_`, `MAIL_`), groups
    /// next to each other with the same prefix joined — Laravel's.
    KeyPrefix,
}

/// Registers a framework's section markers (the app, from the modules' contributions).
pub fn register_sections(sections: EnvSections) {
    register(&SECTIONS, sections);
}

/// Every section marker registered.
pub fn sections() -> Vec<EnvSections> {
    all(&SECTIONS)
}

static SECTIONS: OnceLock<Mutex<Vec<EnvSections>>> = OnceLock::new();

/// How a variable's name is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quoting {
    /// In quotes, `'X'` or `"X"`; spaces may stand around the opening (`getenv( 'X' )`,
    /// `env: 'X'`).
    Quoted,
    /// As it is, right after the opening: `process.env.X`, `${X}`, `$X`.
    Bare,
}

/// One way code reads an environment variable: what's written before its name, how the name
/// is written, what may follow it. The constructors say the common shapes:
///
/// | Code | Access |
/// |---|---|
/// | `getenv('X')`, `env('X', 1)` | `EnvAccess::call("getenv")` |
/// | `$_ENV['X']`, `process.env['X']` | `EnvAccess::index("$_ENV")` |
/// | `process.env.X`, `import.meta.env.X` | `EnvAccess::member("process.env")` |
/// | `env: 'X'` (`#[Autowire(env: 'X')]`) | `EnvAccess::named("env")` |
/// | `%env(X)%`, `%env(int:X)%` | `EnvAccess::placeholder("%env(", &[")%"])` |
/// | `${X}`, `${X:-default}` | `EnvAccess::INTERPOLATION` |
/// | `$X` | `EnvAccess::VARIABLE` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvAccess {
    /// What's written first: a word (`getenv`, `$_ENV`, `process.env`) or an opening of
    /// its own (`%env(`, `${`, `$`).
    pub prefix: &'static str,
    /// What follows the prefix before the name: `(`, `[`, `.`, `:` — or nothing.
    pub opening: &'static str,
    pub quoting: Quoting,
    /// The prefix starts a word: no name's character nor member access (`->`, `.`, `::`)
    /// right before it (`getenv(`, not `mygetenv(` nor `$this->getenv(`).
    pub word: bool,
    /// Processors may come before the name, each followed by `:` (`%env(int:X)%`,
    /// `%env(default:fallback:X)%`): the name is the last part.
    pub processors: bool,
    /// What may follow the name (after its quote): one of these; empty: anything but a
    /// name's character.
    pub after: &'static [&'static str],
}

impl EnvAccess {
    const QUOTED: Self = Self {
        prefix: "",
        opening: "",
        quoting: Quoting::Quoted,
        word: true,
        processors: false,
        after: &[],
    };

    /// A function called with the name in quotes: `getenv('X')`, `env('X', default)`.
    pub const fn call(function: &'static str) -> Self {
        Self { prefix: function, opening: "(", ..Self::QUOTED }
    }

    /// An array indexed by the name in quotes: `$_ENV['X']`, `process.env["X"]`.
    pub const fn index(array: &'static str) -> Self {
        Self { prefix: array, opening: "[", after: &["]"], ..Self::QUOTED }
    }

    /// A named argument with the name in quotes: `env: 'X'`.
    pub const fn named(argument: &'static str) -> Self {
        Self { prefix: argument, opening: ":", ..Self::QUOTED }
    }

    /// A member named after the variable: `process.env.X`.
    pub const fn member(object: &'static str) -> Self {
        Self { prefix: object, opening: ".", quoting: Quoting::Bare, ..Self::QUOTED }
    }

    /// After `open`, processors allowed before the name, one of `close` after it:
    /// `%env(int:X)%` (`placeholder("%env(", &[")%"])`).
    pub const fn placeholder(open: &'static str, close: &'static [&'static str]) -> Self {
        Self {
            prefix: open,
            opening: "",
            quoting: Quoting::Bare,
            word: false,
            processors: true,
            after: close,
        }
    }

    /// `${X}`, `${X:-default}`, `${X-default}`, `${X:=default}`, `${X:?error}`…
    pub const INTERPOLATION: Self = Self {
        prefix: "${",
        opening: "",
        quoting: Quoting::Bare,
        word: false,
        processors: false,
        after: &["}", ":-", "-", ":=", "=", ":?", "?", ":+", "+"],
    };

    /// `$X` (a shell's, Docker Compose's).
    pub const VARIABLE: Self = Self {
        prefix: "$",
        opening: "",
        quoting: Quoting::Bare,
        word: false,
        processors: false,
        after: &[],
    };
}

/// How the code of some languages reads environment variables (a language's or a
/// framework's module contributes it). Made with [`EnvReads::new`] and its builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvReads {
    /// The languages (their ids) whose files read them so.
    pub languages: &'static [&'static str],
    /// Only the files with these names (globs with `*`, any case: `compose*.yaml`); empty:
    /// every file of those languages.
    pub files: &'static [&'static str],
    pub accesses: &'static [EnvAccess],
    /// A variable read here that no `.env` file defines (nor documents) is flagged. Off
    /// where most reads are the code's own variables (a shell's `$X`).
    pub flag_undefined: bool,
    /// The names the runtime provides itself (Node's `NODE_ENV`, Vite's `MODE`): never
    /// flagged.
    pub provided: &'static [&'static str],
}

impl EnvReads {
    /// `accesses` in the files of `languages`, flagged when undefined.
    pub const fn new(languages: &'static [&'static str], accesses: &'static [EnvAccess]) -> Self {
        Self { languages, files: &[], accesses, flag_undefined: true, provided: &[] }
    }

    /// Only in the files named so (globs).
    pub const fn in_files(self, files: &'static [&'static str]) -> Self {
        Self { files, ..self }
    }

    /// Never flagged when undefined.
    pub const fn unflagged(self) -> Self {
        Self { flag_undefined: false, ..self }
    }

    /// With the names the runtime provides itself.
    pub const fn providing(self, provided: &'static [&'static str]) -> Self {
        Self { provided, ..self }
    }

    /// Whether a file named `name` (of language `language`) reads variables so.
    pub fn covers(&self, language: &str, name: &str) -> bool {
        self.languages.contains(&language)
            && (self.files.is_empty() || self.files.iter().any(|g| glob_matches(g, name)))
    }
}

/// Registers how some code reads variables (the app, from the modules' contributions).
pub fn register_reads(reads: EnvReads) {
    register(&READS, reads);
}

/// Every way of reading variables registered.
pub fn reads() -> Vec<EnvReads> {
    all(&READS)
}

static READS: OnceLock<Mutex<Vec<EnvReads>>> = OnceLock::new();

/// The order a framework loads a project's `.env` files in, the later ones overriding the
/// earlier (a framework's module contributes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvFileOrder {
    /// The framework (`Symfony`), as the user reads it.
    pub name: &'static str,
    /// A project follows it when one of these files is at its root (`symfony.lock`,
    /// `config/bundles.php`).
    pub markers: &'static [&'static str],
    /// The files' names in the order they're loaded; `{env}` stands for the environment
    /// (`.env.{env}.local`: `.env.prod.local`).
    pub files: &'static [&'static str],
}

impl EnvFileOrder {
    /// Where a file named `name` comes in the order (`None`: it isn't loaded).
    pub fn rank(&self, name: &str) -> Option<usize> {
        self.files.iter().position(|pattern| match pattern.split_once("{env}") {
            None => pattern.eq_ignore_ascii_case(name),
            Some((before, after)) => {
                let lower = name.to_ascii_lowercase();
                lower.len() > before.len() + after.len()
                    && lower.starts_with(&before.to_ascii_lowercase())
                    && lower.ends_with(&after.to_ascii_lowercase())
                    && !lower[before.len()..lower.len() - after.len()].contains('.')
            }
        })
    }
}

/// Registers a framework's load order (the app, from the modules' contributions).
pub fn register_order(order: EnvFileOrder) {
    register(&ORDERS, order);
}

/// Every load order registered.
pub fn orders() -> Vec<EnvFileOrder> {
    all(&ORDERS)
}

static ORDERS: OnceLock<Mutex<Vec<EnvFileOrder>>> = OnceLock::new();

/// A glob with `*` over a file's name, any case.
fn glob_matches(glob: &str, name: &str) -> bool {
    let (glob, name) = (glob.to_ascii_lowercase(), name.to_ascii_lowercase());
    let mut parts = glob.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else { return false };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else { return rest.is_empty() };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

fn register<T: PartialEq>(registry: &OnceLock<Mutex<Vec<T>>>, item: T) {
    if let Ok(mut all) = registry.get_or_init(Mutex::default).lock()
        && !all.contains(&item)
    {
        all.push(item);
    }
}

fn all<T: Clone>(registry: &OnceLock<Mutex<Vec<T>>>) -> Vec<T> {
    registry.get_or_init(Mutex::default).lock().map(|all| all.clone()).unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/unit/env.rs"]
mod tests;
