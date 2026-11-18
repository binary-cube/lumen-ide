# Release build for Windows, directly on Windows (needs Rust: https://rustup.rs).
#   powershell -ExecutionPolicy Bypass -File scripts\build.ps1
# Result: dist\<id>-<version>-windows-x64.zip (the name from brand.toml).
# Without the language servers (downloaded by the app when first needed, plan 43i); with
# -WithServers (or EDITOR_WITH_SERVERS=1) a full, offline build carrying them in lsp\.
# -Fast: a build to try things on (the `dist-fast` profile: thin LTO, the code generated in
# parallel; faster to build, a larger binary), as `build.sh --fast`.
# -Installer: the installer too, dist\<id>-<version>-windows-x64-setup.exe (Inno Setup 6:
# packaging\windows\installer.iss), beside the zip (both offered: the zip as it is, the
# installer for the Start menu and Settings › Apps).
# -Exe <path>: the program to package instead of building one (the release's, signed).
param([switch]$WithServers, [switch]$Fast, [switch]$Installer, [string]$Exe)

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

$exe = Join-Path $root "target\$cargoProfile\$id.exe"
if ($Exe) {
    $exe = (Resolve-Path $Exe).Path
    Write-Host "$name $version - Windows x64, packaging $exe" -ForegroundColor Cyan
} else {
    Write-Host "$name $version - build Windows x64 ($cargoProfile)" -ForegroundColor Cyan
    Push-Location $root
    try {
        cargo build --profile $cargoProfile
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
}

$zip = Join-Path $dist "$id-$version-windows-x64.zip"
if (Test-Path $zip) { Remove-Item $zip }
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

if ($Installer) {
    # Inno Setup 6, where it installs (choco / its installer); on PATH otherwise.
    $iscc = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $iscc) { $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
    if (-not $iscc) { throw "Inno Setup 6 is missing (https://jrsoftware.org/isdl.php, or: choco install innosetup)" }
    # Windows' version fields take numbers only (1.0.0 for 1.0.0-beta.5).
    $numeric = ($version -split '-')[0]
    $publisher = Brand "publisher"
    $url = "https://github.com/$(Brand "repository")"
    $setup = "$id-$version-windows-x64-setup"
    & $iscc /Q `
        "/DAppName=$name" "/DAppId=$id" "/DAppVersion=$version" "/DNumericVersion=$numeric" `
        "/DPublisher=$publisher" "/DUrl=$url" "/DSourceExe=$exe" `
        "/DIconFile=$(Join-Path $root 'assets\app-icon.ico')" "/DOutputDir=$dist" "/DOutputName=$setup" `
        (Join-Path $root "packaging\windows\installer.iss")
    if ($LASTEXITCODE -ne 0) { throw "the installer couldn't be made" }
    Write-Host "done: $(Join-Path $dist "$setup.exe")" -ForegroundColor Green
}
