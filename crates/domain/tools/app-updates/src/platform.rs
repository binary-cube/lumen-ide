//! The release files a system can run, best first, by the names the release workflow gives
//! them (`<id>-<version>-<platform>.<zip|tar.gz>`).

/// The platforms a build for `os` (`std::env::consts::OS`) on `arch` (`…::ARCH`) can run,
/// best first: a Mac takes its own architecture's build, else the universal one; empty for
/// a system releases aren't made for.
pub fn preferred(os: &str, arch: &str) -> &'static [&'static str] {
    match (os, arch) {
        ("macos", "aarch64") => &["macos-arm64", "macos-universal"],
        ("macos", "x86_64") => &["macos-x86_64", "macos-universal"],
        ("windows", "x86_64") => &["windows-x64"],
        ("linux", "x86_64") => &["linux-x86_64"],
        _ => &[],
    }
}

#[cfg(test)]
#[path = "../tests/unit/platform.rs"]
mod tests;
