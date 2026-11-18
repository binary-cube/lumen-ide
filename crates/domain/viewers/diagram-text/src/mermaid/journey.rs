//! `journey`: sections of tasks, each with a score (1–5: how it felt) and who's in it.

use super::{Error, Line, Meta};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub score: u8,
    pub actors: Vec<String>,
    pub section: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Journey {
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    /// Everyone named, in the order they first appear.
    pub actors: Vec<String>,
}

pub(super) fn parse(lines: &[Line<'_>], meta: &mut Meta) -> Result<Journey, Error> {
    let mut j = Journey::default();
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(title) = t.strip_prefix("title ") {
            meta.title = Some(title.trim().to_owned());
            continue;
        }
        if let Some(section) = t.strip_prefix("section ") {
            j.sections.push(section.trim().to_owned());
            continue;
        }
        let mut parts = t.splitn(3, ':');
        let (Some(name), Some(score)) = (parts.next(), parts.next()) else {
            return Err(Error::unexpected(line));
        };
        let score: u8 = score.trim().parse().map_err(|_| Error::unexpected(line))?;
        let actors: Vec<String> = parts
            .next()
            .map(|a| a.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        for a in &actors {
            if !j.actors.contains(a) {
                j.actors.push(a.clone());
            }
        }
        j.tasks.push(Task {
            name: name.trim().to_owned(),
            score: score.clamp(1, 5),
            actors,
            section: j.sections.len().checked_sub(1),
        });
    }
    Ok(j)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/journey.rs"]
mod tests;
