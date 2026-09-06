# The PowerShell twin of sidecar.sh, for a Windows machine without Git Bash: copies the
# engine built by `cargo build --release -p ulpf` at the repo root to the name Tauri's
# externalBin expects, binaries\ulpf-<host triple>.exe.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$triple = (rustc -vV | Select-String '^host: ').ToString().Substring(6).Trim()

# A generated parser inside the bundle is a broken demo: the app would arrive already
# knowing the unseen format and could raise no proposal of its own.
$generated = Get-ChildItem (Join-Path $root "parsers") -Filter *.toml -ErrorAction SilentlyContinue |
  Where-Object { (Get-Content $_.FullName) -match '^\s*origin.*inferred' }
if ($generated) {
  $generated | ForEach-Object { Write-Host "  $($_.FullName)" }
  throw "sidecar.ps1: the bundle would carry a generated parser (listed above); remove it with: ulpf demo --reset"
}

$src = Join-Path $root "target\release\ulpf.exe"
$dst = Join-Path $root "app\src-tauri\binaries\ulpf-$triple.exe"
if (-not (Test-Path $src)) { throw "sidecar.ps1: $src missing; run: cargo build --release -p ulpf" }
New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
Copy-Item $src $dst -Force
Write-Output "sidecar: $dst"
