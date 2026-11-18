# Release build for Windows, directly on Windows (needs Rust: https://rustup.rs).
#   powershell -ExecutionPolicy Bypass -File scripts\build.ps1
# Result: dist\<id>-<version>-windows-x64.zip (the name from brand.toml).
# Without the language servers (downloaded by the app when first needed, plan 43i); with
# -WithServers (or EDITOR_WITH_SERVERS=1) a full, offline build carrying them in lsp\.
# -Fast: a build to try things on (the `dist-fast` profile: thin LTO, the code generated in
# parallel; faster to build, a larger binary), as `build.sh --fast`.
param([switch]$WithServers, [switch]$Fast)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
function Brand($key) {
    (Select-String -Path "$root\brand.toml" -Pattern "^$key\s*=\s*`"(.+)`"" | Select-Object -First 1).Matches[0].Groups[1].Value
}
$name = Brand "name"
$id = Brand "id"
$cargoProfile = if ($Fast) { "dist-fast" } else { "release" }
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Write-Host "$name $version - build Windows x64 ($cargoProfile)" -ForegroundColor Cyan
Push-Location $root
try {
    cargo build --profile $cargoProfile
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$zip = Join-Path $dist "$id-$version-windows-x64.zip"
if (Test-Path $zip) { Remove-Item $zip }
$exe = Join-Path $root "target\$cargoProfile\$id.exe"
if ($WithServers -or $env:EDITOR_WITH_SERVERS -eq "1") {
    # The language servers beside the .exe (plan 43: Node and the servers, packaging/lsp.toml).
    $lsp = Join-Path $root "target\$cargoProfile\lsp"
    cargo xtask lsp-bundle --platform windows-x64 --out $lsp
    if ($LASTEXITCODE -ne 0) { throw "the language servers couldn't be bundled" }
    Compress-Archive -Path $exe, $lsp -DestinationPath $zip
} else {
    Compress-Archive -Path $exe -DestinationPath $zip
}
Write-Host "done: $zip" -ForegroundColor Green
