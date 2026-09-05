# The PowerShell twin of sidecar.sh, for a Windows machine without Git Bash: copies the
# engine built by `cargo build --release -p ulpf` at the repo root to the name Tauri's
# externalBin expects, binaries\ulpf-<host triple>.exe.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$triple = (rustc -vV | Select-String '^host: ').ToString().Substring(6).Trim()
$src = Join-Path $root "target\release\ulpf.exe"
$dst = Join-Path $root "app\src-tauri\binaries\ulpf-$triple.exe"
if (-not (Test-Path $src)) { throw "sidecar.ps1: $src missing; run: cargo build --release -p ulpf" }
New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
Copy-Item $src $dst -Force
Write-Output "sidecar: $dst"
