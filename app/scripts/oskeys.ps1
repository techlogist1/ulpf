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
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
}
'@
}

# Keystrokes are injected with keybd_event, the same hardware-input layer as mouse_event
# above, not with SendKeys. SendKeys posts WM_CHAR to the focused control of the top-level
# window; WebView2 renders its content in a separate browser process that never sees those
# messages, so on a runner every SendKeys landed nowhere (keydown seen=false) while the mouse
# went straight through. keybd_event enters the same input queue Chromium reads, so the key
# reaches the web content the way a real key does. Only the small SendKeys subset this driver
# sends is translated: single characters, {ESC}/{ENTER}/{TAB}, and the modifier prefixes
# ^ (Ctrl), + (Shift), % (Alt), each applying to the one key token that follows.
$MODVK = @{ '^' = 0x11; '+' = 0x10; '%' = 0x12 }   # VK_CONTROL, VK_SHIFT, VK_MENU
$NAMEVK = @{ 'ESC' = 0x1B; 'ENTER' = 0x0D; 'TAB' = 0x09 }
function Vk-Of([string]$ch) {
  # @(virtual-key, needs-shift) for one character, or $null if unmapped.
  if ($ch -cmatch '^[0-9]$') { return @([int][char]$ch, $false) }
  if ($ch -cmatch '^[a-z]$') { return @([int][char]([string]$ch).ToUpper(), $false) }
  if ($ch -cmatch '^[A-Z]$') { return @([int][char]$ch, $true) }
  switch ($ch) {
    '?' { return @(0xBF, $true) }   # Shift + VK_OEM_2 (the /? key, US layout)
    '/' { return @(0xBF, $false) }
    default { return $null }
  }
}
function Press-Vk([int]$vk, [int[]]$mods, [bool]$shift) {
  $down = @($mods)
  if ($shift -and ($mods -notcontains 0x10)) { $down += 0x10 }
  foreach ($m in $down) { [Win]::keybd_event([byte]$m, 0, 0, [IntPtr]::Zero) }
  Start-Sleep -Milliseconds 12
  [Win]::keybd_event([byte]$vk, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 12
  [Win]::keybd_event([byte]$vk, 0, 2, [IntPtr]::Zero)   # KEYEVENTF_KEYUP
  Start-Sleep -Milliseconds 12
  [array]::Reverse($down)
  foreach ($m in $down) { [Win]::keybd_event([byte]$m, 0, 2, [IntPtr]::Zero) }
}
function Send-KeysHw([string]$s) {
  $i = 0
  while ($i -lt $s.Length) {
    $mods = @()
    while ($i -lt $s.Length -and $MODVK.ContainsKey([string]$s[$i])) { $mods += $MODVK[[string]$s[$i]]; $i++ }
    if ($i -ge $s.Length) { break }
    if ($s[$i] -eq '{') {
      $close = $s.IndexOf('}', $i)
      if ($close -lt 0) { break }
      $name = $s.Substring($i + 1, $close - $i - 1).ToUpper()
      $i = $close + 1
      if ($NAMEVK.ContainsKey($name)) { Press-Vk $NAMEVK[$name] $mods $false }
      elseif ($name.Length -eq 1) { $r = Vk-Of ([string]$name); if ($r) { Press-Vk $r[0] $mods $r[1] } }
    } else {
      $r = Vk-Of ([string]$s[$i]); $i++
      if ($r) { Press-Vk $r[0] $mods $r[1] }
    }
    Start-Sleep -Milliseconds 20
  }
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
if ($Keys) { Send-KeysHw $Keys }
exit 0
