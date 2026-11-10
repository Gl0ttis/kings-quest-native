param([string]$Dir, [string]$Vm, [string]$Out, [int]$Seconds = 100, [string]$Enters = "50,56,62", [int]$UpAt = 75, [int]$From = 70)
# Run 86Box (app\86Box.exe under $Dir) with the VM folder $Vm: press Enter at the seconds
# in $Enters (through the title screens), Up once at $UpAt seconds, and from $From
# seconds capture 86Box's window about 10 times a second to $Out\<ms>.png. The key
# times go to $Out\keys.log.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class W {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
Add-Type -AssemblyName System.Drawing
[W]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out | Remove-Item -Force
function Key($h, [byte]$vk, [byte]$scan, [uint32]$ext) {
    [W]::SetForegroundWindow($h) | Out-Null
    [W]::keybd_event($vk, $scan, $ext, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [W]::keybd_event($vk, $scan, $ext -bor 2, [UIntPtr]::Zero)
}
# The window, drawn into a bitmap by itself (not a copy of the screen).
function Shot($h, $path) {
    $r = New-Object W+RECT
    [W]::GetWindowRect($h, [ref]$r) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc(); [W]::PrintWindow($h, $hdc, 2) | Out-Null; $g.ReleaseHdc($hdc)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
}
$ent = $Enters.Split(',') | ForEach-Object { [int]$_ }
$p = Start-Process -FilePath (Join-Path $Dir "app\86Box.exe") -ArgumentList @("-P", $Vm, "-N") -PassThru
$sw = [Diagnostics.Stopwatch]::StartNew()
while ($p.MainWindowHandle -eq 0 -and $sw.ElapsedMilliseconds -lt 20000) { Start-Sleep -Milliseconds 100; $p.Refresh() }
$h = $p.MainWindowHandle
$log = @(); $done = @{}; $upDone = $false
while ($sw.ElapsedMilliseconds -lt $Seconds * 1000) {
    $t = $sw.ElapsedMilliseconds
    foreach ($e in $ent) { if ($t -ge $e * 1000 -and -not $done[$e]) { Key $h 0x0D 0x1C 0; $done[$e] = 1; $log += "enter $e at $($sw.ElapsedMilliseconds)" } }
    if (-not $upDone -and $t -ge $UpAt * 1000) { Key $h 0x26 0x48 1; $upDone = $true; $log += "up at $($sw.ElapsedMilliseconds)" }
    if ($t -ge $From * 1000) { Shot $h (Join-Path $Out ("{0:D6}.png" -f $t)) }
    Start-Sleep -Milliseconds 70
}
$p | Stop-Process -Force
$log | Set-Content (Join-Path $Out "keys.log")
"done"
