use super::*;

#[test]
fn only_https_from_the_pinned_hosts() {
    for url in [
        "https://nodejs.org/dist/v26.13.0/node.tar.xz",
        "https://registry.npmjs.org/a/-/a-1.0.0.tgz",
        "https://github.com/clangd/clangd/releases/download/23.1.0/clangd-mac-23.1.0.zip",
        "https://objects.githubusercontent.com/github-production-release-asset/1",
        "https://release-assets.githubusercontent.com/github-production-release-asset/1",
        "https://raw.githubusercontent.com/SchemaStore/schemastore/abc/src/x.json",
        "https://dl.static-php.dev/static-php-cli/minimal/php-8.4.22-cli-macos-aarch64.tar.gz",
        "https://static-php-cli.fra1.digitaloceanspaces.com/static-php-cli/minimal/php.tar.gz",
        "https://NODEJS.ORG/dist/x",
    ] {
        assert!(allowed(url), "{url}");
    }
    for url in [
        "http://nodejs.org/dist/x",
        "https://example.com/x",
        "https://nodejs.org.evil.com/x",
        "https://evil.com/nodejs.org/x",
        "https://user@nodejs.org/x",
        "https://nodejs.org:8443/x",
        "file:///etc/passwd",
        "https://",
    ] {
        assert!(!allowed(url), "{url}");
    }
}
