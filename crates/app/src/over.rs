//! What every window shows over itself (a dimmed screen, a calm scene), as the windows and
//! the app's instances tell it one another: the view, and for one on the whole screen the
//! ticket of the window that started it — its screen, when, its process.
//!
//! Of two started on the same screen (two instances left idle at once), the one started
//! first stays and the other gives way ([`Ticket::yields_to`]): both decide the same from
//! what they told each other, nothing else asked. On another screen, both stay.

/// The view over every window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Over {
    pub extension: String,
    /// On the whole screen, started by this ticket's window.
    pub full: Option<Ticket>,
}

/// Who started a view on the whole screen: the screen it's on, when (milliseconds since the
/// epoch) and its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub at_ms: u64,
    pub pid: u32,
    pub display: String,
}

impl Ticket {
    /// One started now by this process on `display`.
    pub fn now(display: String) -> Self {
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        Self { at_ms, pid: std::process::id(), display }
    }

    /// `self` gives way to `other`: on the same screen, started later (at the same moment:
    /// the higher process number).
    pub fn yields_to(&self, other: &Ticket) -> bool {
        self.display == other.display && (other.at_ms, other.pid) < (self.at_ms, self.pid)
    }
}

/// The line telling `over` (`None`: closed): `over <extension>`, `over <extension> full
/// <at> <pid> <display>` (the screen's name last: it may hold spaces), `over`.
pub fn message(over: Option<&Over>) -> String {
    match over {
        Some(Over { extension, full: None }) => format!("over {extension}\n"),
        Some(Over { extension, full: Some(t) }) => {
            let display = t.display.replace(['\r', '\n'], " ");
            format!("over {extension} full {} {} {display}\n", t.at_ms, t.pid)
        }
        None => "over\n".to_owned(),
    }
}

/// What a line says (`None`: not a message of ours).
pub fn parse(line: &str) -> Option<Option<Over>> {
    let rest = line.trim_end_matches(['\r', '\n']).strip_prefix("over")?;
    if rest.is_empty() {
        return Some(None);
    }
    let rest = rest.strip_prefix(' ')?;
    let (extension, full) = match rest.split_once(' ') {
        None => (rest, None),
        Some((extension, tail)) => {
            let mut parts = tail.splitn(4, ' ');
            let ("full", Some(at), Some(pid)) = (parts.next()?, parts.next(), parts.next()) else {
                return None;
            };
            let ticket = Ticket {
                at_ms: at.parse().ok()?,
                pid: pid.parse().ok()?,
                display: parts.next().unwrap_or_default().to_owned(),
            };
            (extension, Some(ticket))
        }
    };
    if extension.is_empty() {
        return None;
    }
    Some(Some(Over { extension: extension.to_owned(), full }))
}

#[cfg(test)]
#[path = "../tests/unit/over.rs"]
mod tests;
