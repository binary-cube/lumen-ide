//! Which archive paths a pattern names: the same names, `*` standing for any one, a folder
//! taking what's in it along. Only `std`: `cargo xtask lsp-bundle` shares the file.

/// Whether `path` (relative, `/`-separated) is what `pattern` names: the same names, `*`
/// standing for any one.
pub fn pruned(pattern: &str, path: &str) -> bool {
    let (mut p, mut s) = (pattern.split('/'), path.split('/'));
    loop {
        match (p.next(), s.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a == "*" || a == b => {}
            _ => return false,
        }
    }
}

/// Whether `path` (relative, `/`-separated) or one of its folders is what one of `patterns`
/// names ([`pruned`]): a pruned folder takes what's in it along.
pub fn pruned_any(patterns: &[String], path: &str) -> bool {
    let mut at = 0;
    loop {
        let end = path[at..].find('/').map_or(path.len(), |i| at + i);
        let prefix = &path[..end];
        if patterns.iter().any(|p| pruned(p, prefix)) {
            return true;
        }
        if end == path.len() {
            return false;
        }
        at = end + 1;
    }
}
