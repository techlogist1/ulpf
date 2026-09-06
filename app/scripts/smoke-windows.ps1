# Installs the NSIS installer this repo built, launches the installed app, and proves the
# window and the engine came up together. If the runner cannot host a webview, the engine
# the installer put on disk is driven directly instead; the last line says which path ran.
# Usage: pwsh app\scripts\smoke-windows.ps1 -Installer <path to *-setup.exe> [-Repo <checkout>]
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  # What `demo --check` reads: samples/, heldout/, parsers/, mappings/ and PROGRESS.md.
  [string]$Repo = (Get-Location).Path
)

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

# The demo's own check mode, run on the installed engine: the inputs, the ports and every
# title and command in PROGRESS.md's demo section. It starts nothing and exits 0 or 1, so it
# belongs here, before the app is launched. This is the shipped binary checking the demo the
# hackathon will run, on Windows, out of the installer.
$out = & $engine demo --check --repo $Repo 2>&1 | Out-String
Write-Host $out
if ($LASTEXITCODE -ne 0) { Fail "the installed engine's demo --check exited $LASTEXITCODE" }

# The installer may have started it already; this job owns the instance it launches.
Get-Process -Name 'ulpf-app', 'ulpf' -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item $urlFile -ErrorAction SilentlyContinue

# WebView2 reads its extra command line from this variable, so the window the job launches
# exposes CDP on 9222 and app/scripts/drive.mjs can measure what the tester actually hit.
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'

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

  # ---- the diagnostic: what the Windows tester reported, measured ------------------------
  # Lag, dead keys, an uneditable review screen and a reset that does nothing were all
  # reported from Windows and are all fine on macOS. This is the only Windows machine we
  # have, so the driver measures them here rather than the report being taken on trust.
  $diag = if ($env:RUNNER_TEMP) { Join-Path $env:RUNNER_TEMP 'ulpf-diagnostic' } else { Join-Path (Get-Location).Path 'diagnostic' }
  New-Item -ItemType Directory -Force -Path $diag | Out-Null
  $procsCsv = Join-Path $diag 'procs.csv'

  # Memory and CPU of the three processes every 10 s, for the whole driver run: "it lags"
  # and "it crashes" are answered by a growing working set as much as by frame gaps.
  $sampler = Start-Job -ArgumentList $procsCsv -ScriptBlock {
    param($csv)
    'time,name,pid,workingsetMB,cpu_s' | Out-File -FilePath $csv -Encoding utf8
    while ($true) {
      foreach ($p in Get-Process -Name 'ulpf-app', 'ulpf', 'msedgewebview2' -ErrorAction SilentlyContinue) {
        $cpu = 0
        try { $cpu = [math]::Round($p.CPU, 2) } catch { $cpu = '' }
        ('{0},{1},{2},{3},{4}' -f (Get-Date -Format o), $p.ProcessName, $p.Id, [math]::Round($p.WorkingSet64 / 1MB, 1), $cpu) |
          Out-File -FilePath $csv -Append -Encoding utf8
      }
      Start-Sleep -Seconds 10
    }
  }

  $driverExit = 0
  if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host '::error::node is not on this runner, so the Windows diagnostic driver could not run (actions/setup-node@v4 is what puts it there)'
    $driverExit = 1
  } else {
    $drive = Join-Path $PSScriptRoot 'drive.mjs'
    Write-Host "driving the running app: $drive"
    # 20 s a screen: the frame percentiles are already stable there, and the OS key runs pay
    # a PowerShell launch each, so this is what keeps the driver inside its own deadline.
    & node $drive --cdp 'http://127.0.0.1:9222' --url-file $urlFile --data $data --repo $Repo --out $diag --secs 20 --pid $proc.Id
    $driverExit = $LASTEXITCODE
  }

  Stop-Job $sampler -ErrorAction SilentlyContinue
  Receive-Job $sampler -ErrorAction SilentlyContinue | Out-Null
  Remove-Job $sampler -Force -ErrorAction SilentlyContinue
  $summary = Join-Path $diag 'summary.txt'
  if (Test-Path $summary) { Write-Host '--- driver summary ---'; Get-Content $summary | Write-Host }
  if (Test-Path $procsCsv) { Write-Host "--- procs.csv, $((Get-Content $procsCsv).Count - 1) samples ---"; Get-Content $procsCsv -Tail 12 | Write-Host }
  Write-Host "driver exit $driverExit"

  # Here, not in the workflow's collect step: every engine start truncates engine.log
  # (src-tauri/src/reset.rs) and the relaunch below is one more start. This is the engine the
  # driver's last reset brought up; the log of the run it measured is engine.log.measured-run,
  # which the driver copied before it reset anything.
  Copy-Item (Join-Path $data 'engine.log') (Join-Path $diag 'engine.log.after-reset') -ErrorAction SilentlyContinue

  # The driver's last act is "reset to first launch", which empties the store, so drop one
  # sample in first: comparing 0 with 0 across the kill would prove nothing.
  # A missing server.url here means the last reset never came back up: say that, rather than
  # keeping the pre-driver URL and blaming the store forty lines further down.
  if (-not (Test-Path $urlFile)) {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Fail "server.url is gone after the driver run: the app did not come back from its last reset"
  }
  $url = (Get-Content $urlFile -Raw).Trim()
  if (-not $url) {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Fail 'server.url is empty after the driver run'
  }
  $sample = Join-Path $Repo 'samples\cisco_asa.log'
  New-Item -ItemType Directory -Force -Path (Join-Path $data 'watch') | Out-Null
  if (Test-Path $sample) { Copy-Item $sample (Join-Path $data 'watch') -ErrorAction SilentlyContinue }
  for ($i = 0; $i -lt 40; $i++) {
    try { if ((Invoke-RestMethod -Uri "$url/api/integrity" -TimeoutSec 2).records -gt 0) { break } } catch { }
    Start-Sleep -Milliseconds 500
  }

  # Read before the kill so the relaunch below can prove the store resumed rather than
  # started over.
  $recordsBefore = -1
  try { $recordsBefore = (Invoke-RestMethod -Uri "$url/api/integrity" -TimeoutSec 5).records } catch { $recordsBefore = -1 }
  Write-Host "records before the kill: $recordsBefore"

  # A hard kill of the window process is NOT the tray's Quit (that runs the shell's exit
  # handler, which kills the child). It is the shape a tester hit: End task on the window
  # left the engine running and the store locked. The job object with
  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE (app/src-tauri/src/job.rs) makes the kernel reap the
  # engine when the app's handles close, however the app died, so an orphan here is now a
  # failure and not a note.
  Stop-Process -Id $proc.Id -Force
  $left = $null
  for ($i = 0; $i -lt 10; $i++) {
    Start-Sleep -Milliseconds 500
    $left = Get-Process -Name 'ulpf' -ErrorAction SilentlyContinue
    if (-not $left) { break }
  }
  if ($left) {
    $left | ForEach-Object { Write-Host "orphan: ulpf.exe pid $($_.Id) outlived a Stop-Process of the window" }
    $left | Stop-Process -Force
    Fail 'the engine outlived a force kill of the window: the kill-on-job-close job did not reap it'
  }
  # The loop breaks on the first empty poll, so say when it was actually empty; 5 s is the
  # ceiling the assertion allows, not the measurement.
  $ms = ($i + 1) * 500
  Write-Host "no ulpf.exe left $ms ms after the window process was force-killed (ceiling 5 s): the job object reaped it"

  # ---- relaunch: the app survives its own force kill --------------------------------------
  # "It crashes" ends here: after the kill the app must come back to the served UI, not to a
  # splash naming a held store, and the append-only store must resume where it was.
  Remove-Item $urlFile -ErrorAction SilentlyContinue
  $proc2 = Start-Process -FilePath $app -PassThru
  $url2 = $null
  for ($i = 0; $i -lt 120; $i++) {
    if (Test-Path $urlFile) { $url2 = (Get-Content $urlFile -Raw).Trim() }
    if ($url2 -or $proc2.HasExited) { break }
    Start-Sleep -Milliseconds 500
  }
  if (-not $url2) { Fail "the relaunched app never wrote server.url (exited=$($proc2.HasExited))" }
  $status2 = $null
  for ($i = 0; $i -lt 40; $i++) {
    try { $status2 = Invoke-RestMethod -Uri "$url2/api/status" -TimeoutSec 2 } catch { $status2 = $null }
    if ($status2 -and $status2.version) { break }
    Start-Sleep -Milliseconds 500
  }
  if (-not ($status2 -and $status2.version)) { Fail "$url2/api/status never answered after the relaunch" }
  Write-Host "relaunched at $url2"

  # The window must be on the served UI. A splash whose fragment starts with '!' is a
  # failure page and '*' is the store-held-by-another-writer page (src/lib.rs): either one
  # means the relaunch did not recover, however healthy /api/status looks.
  $targets = @()
  try { $targets = Invoke-RestMethod -Uri 'http://127.0.0.1:9222/json' -TimeoutSec 5 } catch { $targets = @() }
  $pages = @($targets | Where-Object { $_.type -eq 'page' })
  foreach ($t in $pages) { Write-Host "  target $($t.url)" }
  $onEngine = @($pages | Where-Object { $_.url -like "$url2*" }).Count -gt 0
  $onSplash = @($pages | Where-Object { $_.url -match 'tauri\.localhost' -and ($_.url -match '#!' -or $_.url -match '#\*') }).Count -gt 0
  if ($pages.Count -eq 0) { Write-Host '::error::no CDP page target after the relaunch: WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS did not take' }
  if ($onSplash -or -not $onEngine) { Fail "after the relaunch the window is not on the engine URL $url2 (holder or error splash)" }

  $recordsAfter = -1
  try { $recordsAfter = (Invoke-RestMethod -Uri "$url2/api/integrity" -TimeoutSec 5).records } catch { $recordsAfter = -1 }
  Write-Host "records after the relaunch: $recordsAfter (before the kill: $recordsBefore)"
  if ($recordsBefore -le 0) { Fail 'GET /api/integrity reported no records before the kill, so the store could not be compared' }
  if ($recordsAfter -ne $recordsBefore) { Fail "the store did not resume: $recordsAfter records after the relaunch against $recordsBefore before the kill" }

  Stop-Process -Id $proc2.Id -Force
  $left2 = $null
  for ($i = 0; $i -lt 10; $i++) {
    Start-Sleep -Milliseconds 500
    $left2 = Get-Process -Name 'ulpf' -ErrorAction SilentlyContinue
    if (-not $left2) { break }
  }
  if ($left2) { $left2 | Stop-Process -Force; Fail 'ulpf.exe outlived the second force kill of the window' }
  Write-Host 'the relaunched app came back to the served UI, resumed the store and left no engine behind'

  if ($driverExit -ne 0) { Fail "the Windows diagnostic driver reported failing checks (exit $driverExit); the windows-diagnostic artifact has report.json, the screenshots and procs.csv" }
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
