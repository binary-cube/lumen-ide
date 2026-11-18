//! A URL's parts as requests need them: its scheme, host, port, path (for cookies,
//! redirects, the preview's base). No percent-decoding, no normalization beyond the host's
//! case: what's sent is what was written.

/// A URL split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    /// Lowercase, without the brackets of an IPv6 address.
    pub host: String,
    pub port: Option<u16>,
    /// Starting with `/` (`/` when none was written), without the query.
    pub path: String,
    /// After `?`, without the fragment (`None`: no `?`).
    pub query: Option<String>,
}

impl Url {
    /// `text` split (`None`: not `scheme://host…`).
    pub fn parse(text: &str) -> Option<Self> {
        let (scheme, rest) = text.trim().split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme.is_empty()
            || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            return None;
        }
        let rest = rest.split('#').next().unwrap_or_default();
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        // Credentials in the URL aren't the host.
        let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
        let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
            let (host, after) = inner.split_once(']')?;
            (host, after.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        if host.is_empty() {
            return None;
        }
        let port = match port {
            Some(p) if !p.is_empty() => Some(p.parse().ok()?),
            _ => None,
        };
        let (path, query) = match tail.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (tail, None),
        };
        let path = if path.is_empty() { "/".to_owned() } else { path.to_owned() };
        Some(Self { scheme, host: host.to_ascii_lowercase(), port, path, query })
    }

    /// Its port, written or its scheme's.
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(match self.scheme.as_str() {
            "https" | "wss" => 443,
            _ => 80,
        })
    }

    pub fn is_secure(&self) -> bool {
        matches!(self.scheme.as_str(), "https" | "wss")
    }

    /// `location` (a redirect's, absolute or not) taken from this URL.
    pub fn join(&self, location: &str) -> String {
        let location = location.trim();
        // Absolute: a scheme first (`https://…`), not a `://` further on (`/a?next=https://b`).
        let scheme = location.split_once("://").map(|(scheme, _)| scheme);
        if scheme.is_some_and(|s| {
            s.starts_with(|c: char| c.is_ascii_alphabetic())
                && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }) {
            return location.to_owned();
        }
        let origin = match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme, self.host_text()),
            None => format!("{}://{}", self.scheme, self.host_text()),
        };
        if let Some(rest) = location.strip_prefix("//") {
            return format!("{}://{rest}", self.scheme);
        }
        if location.starts_with('/') {
            return format!("{origin}{location}");
        }
        if location.starts_with('?') {
            return format!("{origin}{}{location}", self.path);
        }
        let dir = self.path.rsplit_once('/').map_or("", |(dir, _)| dir);
        format!("{origin}{dir}/{location}")
    }

    fn host_text(&self) -> String {
        if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() }
    }
}

#[cfg(test)]
#[path = "../tests/unit/url.rs"]
mod tests;
