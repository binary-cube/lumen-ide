//! The variables a Blade template declares before a place: a loop's (only inside it), what
//! `@php` assigns, `@props`. Read from the text, so an unfinished template answers too.

use code_intelligence::template::declared_props;

/// What's visible at a place (each with its `$`).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Visible {
    pub names: Vec<String>,
    pub in_loop: bool,
}

/// The `$name`s in `code`.
fn variables(code: &str) -> impl Iterator<Item = String> + '_ {
    code.match_indices('$').filter_map(|(at, _)| {
        let rest = &code[at + 1..];
        let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
        (end > 0).then(|| format!("${}", &rest[..end]))
    })
}

/// What's inside the bracket at `open`, if it's closed.
fn argument(text: &str, open: usize) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// What's assigned in PHP code.
fn assigned(code: &str, names: &mut Vec<String>) {
    for statement in code.split(';') {
        if let Some((left, _)) = statement.split_once('=')
            && !left.contains(['!', '<', '>'])
        {
            names.extend(variables(left));
        }
    }
}

/// What's visible at `offset` of `text`.
pub(crate) fn visible(text: &str, offset: usize) -> Visible {
    let before = &text[..offset];
    let mut top: Vec<String> = declared_props(before).iter().map(|p| format!("${p}")).collect();
    let mut loops: Vec<Vec<String>> = Vec::new();
    let mut at = 0;
    while let Some(found) = before[at..].find('@') {
        let start = at + found + 1;
        let rest = &before[start..];
        let end = rest.find(|c: char| !c.is_alphanumeric()).unwrap_or(rest.len());
        let open = start + end;
        at = open;
        let bracket = before[open..].starts_with('(');
        match &rest[..end] {
            "foreach" | "forelse" => {
                let given = bracket.then(|| argument(before, open)).flatten();
                let names = given.and_then(|a| a.split_once(" as ")).map_or("", |(_, n)| n);
                loops.push(variables(names).collect());
            }
            "for" | "while" => loops.push(Vec::new()),
            "endforeach" | "endforelse" | "endfor" | "endwhile" => {
                loops.pop();
            }
            "php" if bracket => {
                if let Some(code) = argument(before, open) {
                    assigned(code, &mut top);
                }
            }
            "php" => {
                let code = before[open..].split("@endphp").next().unwrap_or_default();
                assigned(code, &mut top);
            }
            _ => {}
        }
    }
    let in_loop = !loops.is_empty();
    let mut names = top;
    names.extend(loops.into_iter().flatten());
    if in_loop {
        names.push("$loop".to_owned());
    }
    names.sort();
    names.dedup();
    Visible { names, in_loop }
}

#[cfg(test)]
#[path = "../tests/unit/variables.rs"]
mod tests;
