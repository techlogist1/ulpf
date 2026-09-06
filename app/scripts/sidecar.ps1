# The PowerShell twin of sidecar.sh, for a Windows machine without Git Bash: copies the
# engine to the name Tauri's externalBin expects, binaries\ulpf-<host triple>.exe. Same two
# rules as the shell version: the shipped profile (`cargo build --profile dist -p ulpf`)
# with target\release\ as a warned fallback, and CARGO_TARGET_DIR honoured because that is
# where cargo put the build.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$triple = (rustc -vV | Select-String '^host: ').ToString().Substring(6).Trim()

# A generated parser inside the bundle is a broken demo: the app would arrive already
# knowing the unseen format and could raise no proposal of its own.
$generated = Get-ChildItem (Join-Path $root "parsers") -Filter *.toml -ErrorAction SilentlyContinue |
  Where-Object { (Get-Content $_.FullName) -match '^\s*origin.*inferred' }
if ($generated) {
  $generated | ForEach-Object { Write-Host "  $($_.FullName)" }
  throw "sidecar.ps1: the bundle would carry a generated parser (listed above); remove it with: ulpf demo --reset"
}

$src = Join-Path $target "dist\ulpf.exe"
$profileName = "dist"
if (-not (Test-Path $src)) {
  $src = Join-Path $target "release\ulpf.exe"
  $profileName = "release"
  Write-Warning "sidecar.ps1: no $target\dist\ulpf.exe; taking the release profile instead (build the shipped one with: cargo build --profile dist -p ulpf)"
}
if (-not (Test-Path $src)) { throw "sidecar.ps1: $src missing; run: cargo build --profile dist -p ulpf" }

$dst = Join-Path $root "app\src-tauri\binaries\ulpf-$triple.exe"
New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
Copy-Item $src $dst -Force
Write-Output "sidecar: $dst (profile $profileName)"
