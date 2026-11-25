//! An engine's version: `<Chromium's four numbers>-<ours>` (`156.0.8078.100-2`).

/// An engine's version, compared part by part: Chromium's four numbers, then ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    chromium: [u32; 4],
    ours: u32,
}

impl Version {
    /// `156.0.8078.100-2` (`None`: not one). Only its one way of being written is a version
    /// (digits only, no sign, no leading zero), so a version's text names one folder, one
    /// entry of the state, never two spellings of the same.
    pub fn parse(text: &str) -> Option<Self> {
        let (chromium, ours) = text.split_once('-')?;
        let mut parts = chromium.split('.').map(number);
        let mut numbers = [0u32; 4];
        for number in &mut numbers {
            *number = parts.next()??;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Self { chromium: numbers, ours: number(ours)? })
    }
}

/// A number as a version writes it: digits only, no leading zero but for `0` itself.
fn number(text: &str) -> Option<u32> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let canonical = digits && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [major, minor, build, patch] = self.chromium;
        write!(f, "{major}.{minor}.{build}.{patch}-{}", self.ours)
    }
}

#[cfg(test)]
#[path = "../tests/unit/version.rs"]
mod tests;
