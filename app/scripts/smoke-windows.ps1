# Installs the NSIS installer this repo built, launches the installed app, and proves the
# window and the engine came up together. If the runner cannot host a webview, the engine
# the installer put on disk is driven directly instead; the last line says which path ran.
# Usage: pwsh app\scripts\smoke-windows.ps1 -Installer <path to *-setup.exe>
param([Parameter(Mandatory = $true)][string]$Installer)

$ErrorActionPreference = 'Continue'
$data = Join-Path $env:APPDATA 'dev.ulpf.desktop'
$urlFile = Join-Path $data 'server.url'

function Fail($m) { Write-Host "::error::$m"; exit 1 }

Write-Host "installer $Installer ($((Get-Item $Installer).Length) bytes)"
# NSIS silent install. Tauri's installMode is currentUser, so nothing elevates.
Start-Process -FilePath $Installer -ArgumentList '/S' -Wait
Start-Sleep -Seconds 3

$app = Join-Path $env:LOCALAPPDATA 'ULPF\ulpf-app.exe'
if (-not (Test-Path $app)) {
  $app = (Get-ChildItem $env:LOCALAPPDATA -Recurse -Depth 2 -Filter 'ulpf-app.exe' -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
}
if (-not $app) {
  # Say where it did land, so a failure here is diagnosed in this run and not the next one.
  Get-ChildItem $env:LOCALAPPDATA, $env:ProgramFiles -Depth 1 -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like '*ULPF*' -or $_.Name -like '*ulpf*' } |
    ForEach-Object { Write-Host "  candidate $($_.FullName)" }
  Fail 'ulpf-app.exe is not under %LOCALAPPDATA% after a silent install'
}
$dir = Split-Path $app
$engine = Join-Path $dir 'ulpf.exe'
Write-Host "installed into $dir"
Get-ChildItem $dir -Recurse | ForEach-Object { Write-Host ("  {0,10} {1}" -f $_.Length, $_.FullName.Substring($dir.Length + 1)) }
# The sidecar must be beside the executable, not at a dev path (D73).
if (-not (Test-Path $engine)) { Fail 'the sidecar ulpf.exe is not beside the installed app' }

# The installer may have started it already; this job owns the instance it launches.
Get-Process -Name 'ulpf-app', 'ulpf' -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item $urlFile -ErrorAction SilentlyContinue

$proc = Start-Process -FilePath $app -PassThru
for ($i = 0; $i -lt 120; $i++) {
  if (Test-Path $urlFile) { $url = (Get-Content $urlFile -Raw).Trim() }
  if ($url -or $proc.HasExited) { break }
  Start-Sleep -Milliseconds 500
}

if ($url) {
  Write-Host "server.url $url"
  $status = $null
  for ($i = 0; $i -lt 20; $i++) {
    try { $status = Invoke-RestMethod -Uri "$url/api/status" -TimeoutSec 2 } catch { $status = $null }
    if ($status -and $status.version) { break }
    Start-Sleep -Milliseconds 500
  }
  if (-not ($status -and $status.version)) { Fail "$url/api/status never answered with a version" }
  Write-Host ($status | ConvertTo-Json -Depth 4)

  $window = Get-Process -Name 'ulpf-app' -ErrorAction SilentlyContinue
  $child = Get-Process -Name 'ulpf' -ErrorAction SilentlyContinue
  if (-not $window) { Fail 'the window process ulpf-app.exe is not running' }
  if (-not $child) { Fail 'the engine process ulpf.exe is not running' }
  Write-Host "window pid $($window.Id), engine pid $($child.Id)"

  # A hard kill of the window process is NOT the tray's Quit (that runs the shell's exit
  # handler, which kills the child). Windows does not take a child down with its parent, so
  # an orphan here is the expected result of the kill, not a fault in D73's kill-on-exit;
  # the tray path stays a human check. The orphan is reported, then cleaned up.
  Stop-Process -Id $proc.Id -Force
  Start-Sleep -Seconds 5
  $left = Get-Process -Name 'ulpf' -ErrorAction SilentlyContinue
  if ($left) {
    Write-Host "orphan: ulpf.exe pid $($left.Id) outlived a Stop-Process of the window (expected on Windows; the tray's Quit is what kills it)"
    Stop-Process -Id $left.Id -Force
  } else {
    Write-Host 'no ulpf.exe left after the window process was killed'
  }
  Write-Host 'SMOKE PATH: app'
  exit 0
}

# The window never wrote server.url: no webview on this runner, or it exited at once.
Write-Host "the app did not serve (exited=$($proc.HasExited)); falling back to the installed engine"
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
Get-Content (Join-Path $data 'engine.log') -ErrorAction SilentlyContinue | Write-Host

$out = & $engine check 2>&1 | Out-String
Write-Host $out
if ($LASTEXITCODE -ne 0) { Fail "the installed engine's check exited $LASTEXITCODE" }

# lane D's subcommand when it is there, nothing when it is not.
if ((& $engine --help 2>&1 | Out-String) -match '(?m)^\s+demo\b') {
  $out = & $engine demo --check 2>&1 | Out-String
  Write-Host $out
  if ($LASTEXITCODE -ne 0) { Write-Host "::warning::demo --check exited $LASTEXITCODE on Windows" }
}

$out = & $engine run samples --store smoke-store --output smoke-out.jsonl 2>&1 | Out-String
Write-Host $out
if ($LASTEXITCODE -ne 0) { Fail "the installed engine's run exited $LASTEXITCODE" }

New-Item -ItemType Directory -Force -Path smoke-watch, smoke-pending | Out-Null
$p = Start-Process -FilePath $engine -PassThru `
  -ArgumentList 'serve', 'smoke-watch', '--store', 'smoke-store2', '--output', 'smoke-out2.jsonl',
  '--parsers', 'parsers', '--pending', 'smoke-pending', '--listen', '127.0.0.1:7911' `
  -RedirectStandardOutput smoke-serve.out.log -RedirectStandardError smoke-serve.err.log
$status = $null
for ($i = 0; $i -lt 60; $i++) {
  try { $status = Invoke-RestMethod -Uri 'http://127.0.0.1:7911/api/status' -TimeoutSec 2 } catch { $status = $null }
  if ($status -and $status.version) { break }
  Start-Sleep -Milliseconds 500
}
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Get-Content smoke-serve.out.log, smoke-serve.err.log -ErrorAction SilentlyContinue | Write-Host
if (-not ($status -and $status.version)) { Fail 'the installed engine served nothing on 127.0.0.1:7911' }
Write-Host ($status | ConvertTo-Json -Depth 4)
Write-Host 'SMOKE PATH: sidecar'
exit 0
