param([string]$Dir, [int]$Seconds = 75, [int]$UpAt = 45)
# Run 86Box (IBM PCjr, the original KQ disk), press Enter through the title screens, press
# Up once at $UpAt seconds, and capture the screen ~10 times a second, timestamped.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class W {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
Add-Type -AssemblyName System.Drawing
[W]::SetProcessDPIAware() | Out-Null
$frames = Join-Path $Dir "frames"
New-Item -ItemType Directory -Force $frames | Out-Null
Get-ChildItem $frames | Remove-Item -Force
$p = Start-Process -FilePath (Join-Path $Dir "app\86Box.exe") -ArgumentList @("-P", (Join-Path $Dir "vm"), "-N") -PassThru
$sw = [Diagnostics.Stopwatch]::StartNew()
while ($p.MainWindowHandle -eq 0 -and $sw.ElapsedMilliseconds -lt 20000) { Start-Sleep -Milliseconds 100; $p.Refresh() }
$h = $p.MainWindowHandle
"window found at {0} ms" -f $sw.ElapsedMilliseconds
function Key([byte]$vk, [byte]$scan, [uint32]$ext) {
    [W]::SetForegroundWindow($h) | Out-Null
    [W]::keybd_event($vk, $scan, $ext, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [W]::keybd_event($vk, $scan, $ext -bor 2, [UIntPtr]::Zero)
}
$enters = @(8, 11, 14, 17, 20, 23, 26, 29, 32, 35, 38)
$log = @()
$upDone = $false
while ($sw.ElapsedMilliseconds -lt $Seconds * 1000) {
    $t = $sw.ElapsedMilliseconds
    foreach ($e in $enters) {
        if ($t -ge $e * 1000 -and -not ($log -match "enter $e ")) { Key 0x0D 0x1C 0; $log += "enter $e at $($sw.ElapsedMilliseconds)" }
    }
    if (-not $upDone -and $t -ge $UpAt * 1000) {
        Key 0x26 0x48 1
        $upDone = $true
        $log += "up at $($sw.ElapsedMilliseconds)"
    }
    $r = New-Object W+RECT
    [W]::GetClientRect($h, [ref]$r) | Out-Null
    $pt = New-Object W+POINT
    [W]::ClientToScreen($h, [ref]$pt) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -gt 0 -and $hh -gt 0) {
        $bmp = New-Object System.Drawing.Bitmap $w, $hh
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($pt.X, $pt.Y, 0, 0, $bmp.Size)
        $bmp.Save((Join-Path $frames ("{0:D6}.png" -f $t)), [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
    }
    Start-Sleep -Milliseconds 70
}
$log | Set-Content (Join-Path $Dir "keys.log")
$log
Stop-Process -Id $p.Id -Force

