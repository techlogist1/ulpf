# OS-level keystrokes and mouse clicks aimed at one process's main window. The Windows
# measurement job needs the real input path -- the window manager, focus, WebView2's own
# accelerator handling -- which CDP's page.keyboard bypasses entirely. That difference is
# the whole diagnosis when a tester says "the number keys do nothing".
#   pwsh app\scripts\oskeys.ps1 -Pid 1234 -Keys '{ESC}'
#   pwsh app\scripts\oskeys.ps1 -Pid 1234 -Click '800,400'    # screen coordinates
#   pwsh app\scripts\oskeys.ps1 -Pid 1234 -Rect               # window rect as JSON
# SendKeys syntax: {ESC} {ENTER} {TAB}, ^ = Ctrl, + = Shift, % = Alt, so Ctrl+Shift+R is
# '^+r'. Literal + ^ % ~ ( ) [ ] { } must be wrapped in braces by the caller.
# Exit codes: 0 sent, 2 bad arguments, 3 Windows refused the foreground (the keys went
# somewhere else, which is a different diagnosis from "the UI ignored them").
param(
  # $Pid is a read-only automatic variable, so the parameter cannot be named Pid; the alias
  # keeps the documented -Pid spelling on the command line.
  [Parameter(Mandatory = $true)][Alias('Pid')][int]$TargetPid,
  [string]$Keys,
  # One string, not [int[]]: `-Click 800 400` would bind 800 to Click and 400 positionally
  # to -Keys, and SendKeys would type "400" into the app instead of clicking it.
  [string]$Click,
  [switch]$Rect
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms

# Guarded so a warm process does not recompile it; each run is its own process, so this is
# only correctness insurance against a caller dot-sourcing the script twice.
if (-not ('Win' -as [type])) {
  Add-Type @'
using System;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public static class Win {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
}
'@
}

$xy = @()
if ($Click) {
  $xy = @($Click -split ',' | ForEach-Object { [int]$_.Trim() })
  if ($xy.Count -ne 2) { [Console]::Error.WriteLine("-Click needs 'X,Y' in screen coordinates, got '$Click'"); exit 2 }
}

$proc = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
if (-not $proc) { [Console]::Error.WriteLine("no process with pid $TargetPid"); exit 2 }
$h = $proc.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { [Console]::Error.WriteLine("pid $TargetPid has no main window"); exit 2 }

# SW_RESTORE first: a minimized window refuses the foreground, swallows every key, and
# reports a rect of (-32000,-32000), so this has to happen before -Rect answers too.
[void][Win]::ShowWindow($h, 9)

if ($Rect) {
  $r = New-Object RECT
  [void][Win]::GetWindowRect($h, [ref]$r)
  @{ left = $r.Left; top = $r.Top; right = $r.Right; bottom = $r.Bottom } | ConvertTo-Json -Compress
  exit 0
}

# Windows refuses SetForegroundWindow to a process that is not itself foreground and holds
# no recent input. Silence there looks exactly like a UI that ignored the key, so the
# refusal is reported rather than discarded.
$set = [Win]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 250
$fg = [Win]::GetForegroundWindow()
@{ set_foreground = [bool]$set; window = [int64]$h; foreground = [int64]$fg } | ConvertTo-Json -Compress

if ($xy.Count -eq 2) {
  [void][Win]::SetCursorPos($xy[0], $xy[1])
  Start-Sleep -Milliseconds 80
  [Win]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)   # LEFTDOWN
  Start-Sleep -Milliseconds 40
  [Win]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)   # LEFTUP
  Start-Sleep -Milliseconds 120
  $fg = [Win]::GetForegroundWindow()   # the click itself grants the foreground
}

# Read again right before sending: SendKeys goes to whatever holds the foreground, so a
# refused activation must stop the send rather than type into another window.
$fg = [Win]::GetForegroundWindow()
if ($fg -ne $h) { [Console]::Error.WriteLine("the foreground is $fg, not the app's window $h (SetForegroundWindow=$set): the keys were not sent"); exit 3 }
if ($Keys) { [System.Windows.Forms.SendKeys]::SendWait($Keys) }
exit 0
