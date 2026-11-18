//! A project's cookies: what servers set (`Set-Cookie`), sent back where they belong
//! (RFC 6265: the domain, the path, `Secure`, expiry), kept between runs as lines of text
//! (the module writes them in the project's own folder, never committed). The time is the
//! caller's (seconds since the epoch): the domain reads no clock.

use crate::url::Url;

/// A cookie kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// The host it was set by, or the domain it names (without a leading dot).
    pub domain: String,
    /// Sent to the domain's subdomains too (it named a `Domain`).
    pub subdomains: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// When it ends (seconds since the epoch; `None`: with the session — kept until the
    /// jar is cleared).
    pub expires: Option<u64>,
}

/// The cookies of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl CookieJar {
    pub fn cookies(&self) -> &[Cookie] {
        &self.cookies
    }

    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// What a run changed in its copy (`before` → `after`: cookies set, replaced, removed)
    /// applied here, the rest kept: two runs at once each add theirs, neither undoes the
    /// other's. `true`: something changed.
    pub fn apply_changes(&mut self, before: &Self, after: &Self) -> bool {
        let same =
            |a: &Cookie, b: &Cookie| a.name == b.name && a.domain == b.domain && a.path == b.path;
        let mut changed = false;
        for gone in before.cookies.iter().filter(|b| !after.cookies.iter().any(|a| same(a, b))) {
            let len = self.cookies.len();
            self.cookies.retain(|c| !same(c, gone));
            changed |= self.cookies.len() != len;
        }
        for set in after.cookies.iter().filter(|a| !before.cookies.contains(a)) {
            if self.cookies.iter().any(|c| c == set) {
                continue;
            }
            self.cookies.retain(|c| !same(c, set));
            self.cookies.push(set.clone());
            changed = true;
        }
        changed
    }

    /// A response from `url` set `header` (one `Set-Cookie`'s value) at `now`.
    pub fn store(&mut self, url: &Url, header: &str, now: u64) {
        let mut parts = header.split(';');
        let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else { return };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let mut cookie = Cookie {
            name: name.to_owned(),
            value: value.trim().trim_matches('"').to_owned(),
            domain: url.host.clone(),
            subdomains: false,
            path: default_path(&url.path),
            secure: false,
            http_only: false,
            expires: None,
        };
        let mut max_age = None;
        for attribute in parts {
            let (key, value) = attribute.split_once('=').unwrap_or((attribute, ""));
            let value = value.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "domain" => {
                    let domain = value.trim_start_matches('.').to_ascii_lowercase();
                    // A domain that isn't the host's own or one above it is refused.
                    if !domain.is_empty() && domain_matches(&url.host, &domain) {
                        cookie.domain = domain;
                        cookie.subdomains = true;
                    } else if !domain.is_empty() {
                        return;
                    }
                }
                "path" if value.starts_with('/') => cookie.path = value.to_owned(),
                "secure" => cookie.secure = true,
                "httponly" => cookie.http_only = true,
                "max-age" => max_age = value.parse::<i64>().ok(),
                "expires" if max_age.is_none() => cookie.expires = http_date(value),
                _ => {}
            }
        }
        if let Some(age) = max_age {
            cookie.expires = Some(if age <= 0 { 0 } else { now.saturating_add(age as u64) });
        }
        self.cookies.retain(|c| {
            !(c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path)
        });
        // Expired already: it's deleted (what a server does to remove one).
        if cookie.expires.is_none_or(|end| end > now) {
            self.cookies.push(cookie);
        }
    }

    /// The `Cookie` header for a request to `url` at `now` (`None`: none to send).
    pub fn header_for(&self, url: &Url, now: u64) -> Option<String> {
        let mut sent: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| c.expires.is_none_or(|end| end > now))
            .filter(|c| {
                if c.subdomains {
                    domain_matches(&url.host, &c.domain)
                } else {
                    url.host == c.domain
                }
            })
            .filter(|c| path_matches(&url.path, &c.path))
            .filter(|c| !c.secure || url.is_secure())
            .collect();
        // The longer paths first (RFC 6265 5.4).
        sent.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        let header: Vec<String> = sent.iter().map(|c| format!("{}={}", c.name, c.value)).collect();
        (!header.is_empty()).then(|| header.join("; "))
    }

    /// Kept as text: one cookie a line, its fields tab-separated.
    pub fn to_text(&self) -> String {
        self.cookies
            .iter()
            .map(|c| {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    c.domain,
                    u8::from(c.subdomains),
                    c.path,
                    u8::from(c.secure),
                    u8::from(c.http_only),
                    c.expires.map_or_else(String::new, |e| e.to_string()),
                    c.name,
                    c.value
                )
            })
            .collect()
    }

    /// What [`Self::to_text`] wrote, without the ones ended by `now` (a line it can't read
    /// is skipped).
    pub fn from_text(text: &str, now: u64) -> Self {
        let cookies = text
            .lines()
            .filter_map(|line| {
                let f: Vec<&str> = line.splitn(8, '\t').collect();
                let [domain, subdomains, path, secure, http_only, expires, name, value] = f[..]
                else {
                    return None;
                };
                let expires = if expires.is_empty() { None } else { Some(expires.parse().ok()?) };
                Some(Cookie {
                    name: name.to_owned(),
                    value: value.to_owned(),
                    domain: domain.to_owned(),
                    subdomains: subdomains == "1",
                    path: path.to_owned(),
                    secure: secure == "1",
                    http_only: http_only == "1",
                    expires,
                })
            })
            .filter(|c| c.expires.is_none_or(|end| end > now))
            .collect();
        Self { cookies }
    }
}

/// `host` is `domain` or below it.
fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

/// A request's `path` is under a cookie's `cookie` path.
fn path_matches(path: &str, cookie: &str) -> bool {
    path == cookie
        || (path.starts_with(cookie)
            && (cookie.ends_with('/') || path[cookie.len()..].starts_with('/')))
}

/// The path a cookie gets when it names none: the request's, up to its last `/`.
fn default_path(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(at) => path[..at].to_owned(),
    }
}

/// An HTTP date (`Wed, 21 Oct 2026 07:28:00 GMT`) in seconds since the epoch.
pub fn http_date(text: &str) -> Option<u64> {
    const MONTHS: [&str; 12] =
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let words: Vec<&str> = text.split([' ', ',', '-']).filter(|w| !w.is_empty()).collect();
    let mut day = None;
    let mut month = None;
    let mut year = None;
    let mut time = None;
    for word in words {
        let lower = word.to_ascii_lowercase();
        if word.contains(':') {
            let mut t = word.split(':').map(|n| n.parse::<u64>().ok());
            time = Some((t.next()??, t.next()??, t.next().flatten().unwrap_or(0)));
        } else if let Some(m) = MONTHS.iter().position(|m| lower.starts_with(m)) {
            month = Some(m as u64 + 1);
        } else if let Ok(n) = word.parse::<u64>() {
            if day.is_none() && n <= 31 && year.is_none() {
                day = Some(n);
            } else {
                year = Some(if n < 70 {
                    2000 + n
                } else if n < 100 {
                    1900 + n
                } else {
                    n
                });
            }
        }
    }
    let (y, m, d) = (year?, month?, day?);
    let (hh, mm, ss) = time.unwrap_or((0, 0, 0));
    // A civil date to days (Howard Hinnant's algorithm).
    let (y, m) = (y as i64 - i64::from(m <= 2), m as i64);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = u64::try_from(days).ok()?.checked_mul(86_400)?;
    seconds.checked_add(hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
#[path = "../tests/unit/cookies.rs"]
mod tests;
