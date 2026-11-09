param([string]$Dir, [string]$Vm, [int]$Seconds = 110, [string]$Enters = "50,56,62,68,74,80,86")
# Run 86Box (app\86Box.exe under $Dir) with the VM folder $Vm for $Seconds, pressing Enter
# at the seconds listed in $Enters (through the title screens), and every 4 s take 86Box's
# own screenshot (Ctrl+F11): the emulated screen at its resolution, saved into
# $Vm\screenshots\.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class W {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
"@
[W]::SetProcessDPIAware() | Out-Null
function Press($h, [byte[]]$vks, [byte[]]$scans, [uint32[]]$ext) {
    [W]::SetForegroundWindow($h) | Out-Null
    for ($i = 0; $i -lt $vks.Length; $i++) { [W]::keybd_event($vks[$i], $scans[$i], $ext[$i], [UIntPtr]::Zero) }
    Start-Sleep -Milliseconds 70
    for ($i = $vks.Length - 1; $i -ge 0; $i--) { [W]::keybd_event($vks[$i], $scans[$i], $ext[$i] -bor 2, [UIntPtr]::Zero) }
}
$ent = @(); if ($Enters) { $ent = $Enters.Split(',') | ForEach-Object { [int]$_ } }
$p = Start-Process -FilePath (Join-Path $Dir "app\86Box.exe") -ArgumentList @("-P", $Vm, "-N") -PassThru
$sw = [Diagnostics.Stopwatch]::StartNew()
while ($p.MainWindowHandle -eq 0 -and $sw.ElapsedMilliseconds -lt 20000) { Start-Sleep -Milliseconds 100; $p.Refresh() }
$h = $p.MainWindowHandle
$next = 4; $done = @{}
while ($sw.ElapsedMilliseconds -lt $Seconds * 1000) {
    $s = [int]($sw.ElapsedMilliseconds / 1000)
    foreach ($e in $ent) { if ($s -ge $e -and -not $done[$e]) { Press $h @(0x0D) @(0x1C) @(0); $done[$e] = 1 } }
    if ($sw.ElapsedMilliseconds -ge $next * 1000) { Press $h @(0x11, 0x7A) @(0x1D, 0x57) @(0, 0); $next += 4 }
    Start-Sleep -Milliseconds 100
}
$p | Stop-Process -Force
"done"
