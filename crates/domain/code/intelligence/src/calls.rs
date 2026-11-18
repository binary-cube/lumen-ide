//! What a framework says of its calls' results, as data the language's inference reads: a
//! call whose result is the class an argument names — Symfony's and PSR's
//! `$container->get(Mailer::class)` gives a `Mailer`, where the method's own type says only
//! `object` / `mixed` — or, given a name (`$container->get('mailer')`), the class the
//! project's table of such names says ([`crate::SymbolIndex::named_type`]: filled by the
//! framework from the project's own files). Registered once by the app from the modules'
//! contributions (they don't depend on the language's module, nor it on theirs); the rule is
//! the same in every project, the names are each project's.

use std::sync::RwLock;

/// Calling `method` of `class` (or of a class that extends it without declaring the method
/// again) gives the class named by its argument at `argument` (`X::class`), or — a name
/// given — the class table `names` of the project's index has for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassArgumentCall {
    /// The class declaring the method (fully qualified, no leading `\`).
    pub class: &'static str,
    pub method: &'static str,
    /// The argument's place (0: the first).
    pub argument: usize,
    /// The table of the project's index a name given there is looked up in (`None`: only
    /// `X::class` is understood).
    pub names: Option<&'static str>,
}

impl ClassArgumentCall {
    /// `method` of `class` gives the class its argument at `argument` names.
    pub const fn new(class: &'static str, method: &'static str, argument: usize) -> Self {
        Self { class, method, argument, names: None }
    }

    /// A name given there is looked up in the project's table `names`.
    pub const fn names(mut self, names: &'static str) -> Self {
        self.names = Some(names);
        self
    }
}

static CALLS: RwLock<Vec<ClassArgumentCall>> = RwLock::new(Vec::new());

/// Registers a framework's call (the app, from the modules' contributions).
pub fn register(call: ClassArgumentCall) {
    if let Ok(mut all) = CALLS.write()
        && !all.contains(&call)
    {
        all.push(call);
    }
}

/// What a framework said of calling `method` declared by `class` (case-insensitive, as
/// PHP's names): `None` if nothing. Cheap: asked at each call inferred.
pub fn class_argument(class: &str, method: &str) -> Option<ClassArgumentCall> {
    let class = class.trim_start_matches('\\');
    let all = CALLS.read().ok()?;
    all.iter()
        .find(|c| c.class.eq_ignore_ascii_case(class) && c.method.eq_ignore_ascii_case(method))
        .copied()
}

#[cfg(test)]
#[path = "../tests/unit/calls.rs"]
mod tests;
