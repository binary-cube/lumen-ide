//! cURL in and out: a resolved request as a `curl` command (to share, to run in a terminal),
//! and a `curl` command pasted (from a browser's "Copy as cURL", a docs page) as a request of
//! a `.http` file.

use crate::model::BodyPart;
use crate::vars::ResolvedRequest;

/// `text` quoted for a POSIX shell (single quotes, a single quote inside closed and opened
/// again).
fn quote(text: &str) -> String {
    if !text.is_empty()
        && text.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
    {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `request` as a `curl` command, one option a line.
/// `request` as PHP code sending it with Guzzle ("Convert to PHP / Guzzle").
pub fn to_guzzle(request: &ResolvedRequest) -> String {
    let php = |text: &str| format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"));
    let mut out = String::from("$client = new \\GuzzleHttp\\Client();\n");
    out.push_str(&format!(
        "$response = $client->request({}, {}, [\n",
        php(&request.method),
        php(&request.url)
    ));
    if !request.headers.is_empty() {
        out.push_str("    'headers' => [\n");
        for header in &request.headers {
            out.push_str(&format!("        {} => {},\n", php(&header.name), php(&header.value)));
        }
        out.push_str("    ],\n");
    }
    match request.body.as_slice() {
        [] => {}
        [BodyPart::File(path)] => {
            out.push_str(&format!("    'body' => fopen({}, 'r'),\n", php(path)))
        }
        parts => {
            let text: String = parts
                .iter()
                .map(|p| match p {
                    BodyPart::Text(t) => t.clone(),
                    BodyPart::File(path) => format!("< {path}"),
                })
                .collect();
            out.push_str(&format!("    'body' => {},\n", php(&text)));
        }
    }
    out.push_str("]);\n");
    out
}

pub fn to_curl(request: &ResolvedRequest) -> String {
    let mut parts = vec![format!("curl -X {}", request.method), quote(&request.url)];
    for header in &request.headers {
        parts.push(format!("-H {}", quote(&format!("{}: {}", header.name, header.value))));
    }
    for part in &request.body {
        match part {
            BodyPart::Text(text) => parts.push(format!("--data-raw {}", quote(text))),
            BodyPart::File(path) => {
                parts.push(format!("--data-binary {}", quote(&format!("@{path}"))))
            }
        }
    }
    parts.join(" \\\n  ")
}

/// A shell command's words (quotes, escapes and line continuations as a POSIX shell reads
/// them; `$'…'` too, browsers write it).
fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // A line continued.
                Some('\n') => {}
                Some('\r') => {
                    chars.next_if_eq(&'\n');
                }
                Some(next) => {
                    word.push(next);
                    started = true;
                }
                None => {}
            },
            '\'' => {
                started = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                started = true;
                while let Some(c) = chars.next() {
                    match c {
                        '\'' => break,
                        '\\' => match chars.next() {
                            Some('n') => word.push('\n'),
                            Some('t') => word.push('\t'),
                            Some('r') => word.push('\r'),
                            Some(other) => word.push(other),
                            None => {}
                        },
                        _ => word.push(c),
                    }
                }
            }
            '"' => {
                started = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            if let Some(next) = chars.next() {
                                if !matches!(next, '"' | '\\' | '$' | '`') {
                                    word.push('\\');
                                }
                                word.push(next);
                            }
                        }
                        _ => word.push(c),
                    }
                }
            }
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// `bytes` in Base64 (standard alphabet, padded).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A `curl` command as a request of a `.http` file (`None`: not a `curl` command, or no URL
/// in it). What a request can't say (`--compressed`, `-L`, `-s`…) is left out.
pub fn from_curl(command: &str) -> Option<String> {
    let words = words(command.trim());
    let mut words = words.into_iter();
    if words.next().as_deref() != Some("curl") {
        return None;
    }
    let (mut method, mut url) = (None::<String>, None::<String>);
    let (mut headers, mut data) = (Vec::<String>::new(), Vec::<String>::new());
    let mut get = false;
    // `-d @file`: the body read from that file (`< file` in a request).
    let mut from_file = None;
    while let Some(word) = words.next() {
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) if word.starts_with("--") => {
                (flag.to_owned(), Some(value.to_owned()))
            }
            _ => (word.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| words.next());
        match flag.as_str() {
            "-X" | "--request" => method = value(),
            "-H" | "--header" => headers.extend(value()),
            "-d" | "--data" | "--data-raw" | "--data-ascii" | "--data-binary"
            | "--data-urlencode" => match value() {
                Some(v) if flag != "--data-raw" && v.starts_with('@') => {
                    from_file = Some(v[1..].to_owned());
                }
                v => data.extend(v),
            },
            "--json" => {
                data.extend(value());
                headers.push("Content-Type: application/json".to_owned());
                headers.push("Accept: application/json".to_owned());
            }
            "-u" | "--user" => {
                if let Some(user) = value() {
                    headers.push(format!("Authorization: Basic {}", base64(user.as_bytes())));
                }
            }
            "-A" | "--user-agent" => headers.extend(value().map(|v| format!("User-Agent: {v}"))),
            "-b" | "--cookie" => headers.extend(value().map(|v| format!("Cookie: {v}"))),
            "-e" | "--referer" => headers.extend(value().map(|v| format!("Referer: {v}"))),
            "--url" => url = value(),
            "-G" | "--get" => get = true,
            // Options taking a value that a request doesn't need.
            "-o" | "--output" | "-m" | "--max-time" | "--connect-timeout" | "-x" | "--proxy"
            | "-w" | "--write-out" | "--retry" => {
                value();
            }
            other if other.starts_with('-') => {}
            _ if url.is_none() => url = Some(word),
            _ => {}
        }
    }
    let url = url?;
    let mut url = url;
    let body = match from_file {
        Some(path) => format!("< {path}"),
        None => data.join("&"),
    };
    if get && !body.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&body);
    }
    let has_body = !get && !body.is_empty();
    let method = method.unwrap_or_else(|| if has_body { "POST" } else { "GET" }.to_owned());
    let mut out = format!("{} {url}\n", method.to_ascii_uppercase());
    let typed = headers.iter().any(|h| h.to_ascii_lowercase().starts_with("content-type:"));
    for header in &headers {
        out.push_str(header);
        out.push('\n');
    }
    if has_body {
        if !typed {
            out.push_str("Content-Type: application/x-www-form-urlencoded\n");
        }
        out.push('\n');
        out.push_str(&body);
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
#[path = "../tests/unit/curl.rs"]
mod tests;
