//! The variables a template declares before a place: `{% set %}`, `{% for %}` and a macro's
//! arguments (only inside them), `{% import %}` and `{% from %}`. Read from the text, so an
//! unfinished template answers too.

/// What's visible at a place.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Visible {
    pub names: Vec<String>,
    pub in_loop: bool,
}

/// A block declaring names of its own.
#[derive(Debug)]
struct Scope {
    closes: &'static str,
    names: Vec<String>,
}

fn is_name(word: &str) -> bool {
    !word.is_empty()
        && word.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !word.starts_with(|c: char| c.is_ascii_digit())
}

/// The names of a comma-separated list, each up to a space, `=` or `(`.
fn list(code: &str) -> impl Iterator<Item = String> + '_ {
    code.split(',').filter_map(|part| {
        let name = part.trim().split([' ', '=', '(']).next()?;
        is_name(name).then(|| name.to_owned())
    })
}

/// `a as b, c`'s names (`b`, `c`).
fn imported(code: &str) -> impl Iterator<Item = String> + '_ {
    code.split(',').filter_map(|part| {
        let name = part.rsplit(" as ").next()?.trim();
        is_name(name).then(|| name.to_owned())
    })
}

/// The statement `body` (a tag's content) applied to what's visible.
fn apply(body: &str, top: &mut Vec<String>, scopes: &mut Vec<Scope>) {
    let body = body.trim_matches(['-', '~', ' ', '\n', '\t', '\r']);
    let (tag, rest) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    let rest = rest.trim();
    match tag {
        "set" => {
            let names = rest.split('=').next().unwrap_or(rest);
            let target = scopes.last_mut().map_or(&mut *top, |s| &mut s.names);
            target.extend(list(names));
        }
        "for" => {
            let names = rest.split(" in ").next().unwrap_or_default();
            scopes.push(Scope { closes: "endfor", names: list(names).collect() });
        }
        "macro" => {
            let args = rest.split_once('(').map_or("", |(_, a)| a);
            let args = args.rsplit_once(')').map_or(args, |(a, _)| a);
            scopes.push(Scope { closes: "endmacro", names: list(args).collect() });
        }
        "import" => top.extend(
            rest.rsplit(" as ").next().map(str::trim).filter(|n| is_name(n)).map(str::to_owned),
        ),
        "from" => top
            .extend(rest.split_once(" import ").into_iter().flat_map(|(_, names)| imported(names))),
        "endfor" | "endmacro" if scopes.last().is_some_and(|s| s.closes == tag) => {
            scopes.pop();
        }
        _ => {}
    }
}

/// What's visible at `offset` of `text`.
pub(crate) fn visible(text: &str, offset: usize) -> Visible {
    let before = &text[..offset];
    let (mut top, mut scopes) = (Vec::new(), Vec::new());
    let mut from = 0;
    while let Some(open) = before[from..].find("{%") {
        let start = from + open + 2;
        let Some(close) = before[start..].find("%}") else { break };
        apply(&before[start..start + close], &mut top, &mut scopes);
        from = start + close + 2;
    }
    let in_loop = scopes.iter().any(|s| s.closes == "endfor");
    let mut names: Vec<String> = top;
    names.extend(scopes.into_iter().flat_map(|s| s.names));
    names.sort();
    names.dedup();
    Visible { names, in_loop }
}

#[cfg(test)]
#[path = "../tests/unit/variables.rs"]
mod tests;
