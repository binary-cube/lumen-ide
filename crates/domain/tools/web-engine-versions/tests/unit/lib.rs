use super::*;

/// Each system has its name in the list, found back by it; none for a system no engine is
/// made for.
#[test]
fn each_system_has_its_name() {
    assert_eq!(platform("macos", "aarch64"), Some("macos-arm64"));
    assert_eq!(platform("windows", "x86_64"), Some("windows-x64"));
    assert_eq!(platform("linux", "riscv64"), None);
    for p in &PLATFORMS {
        assert_eq!(platform(p.os, p.arch), Some(p.name));
        assert_eq!(Platform::named(p.name), Some(p));
    }
    assert_eq!(Platform::named("linux-riscv64"), None);
}

/// The host's program where each system's package puts it.
#[test]
fn the_program_is_where_its_package_puts_it() {
    let dir = Path::new("e");
    assert_eq!(program(dir, "macos"), dir.join("web-host.app/Contents/MacOS/web-host"));
    assert_eq!(program(dir, "windows"), dir.join("web-host.exe"));
    assert_eq!(program(dir, "linux"), dir.join("web-host"));
}
