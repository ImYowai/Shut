# Shut Remote 検証用ウィンドウをサブモニターへ移動する
param([string]$TitleLike = "Shut Remote")

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinMover {
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
"@

$proc = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like "*$TitleLike*" } | Select-Object -First 1
if (-not $proc) { Write-Output "no window: $TitleLike"; exit 1 }
$scr = [System.Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
if (-not $scr) { Write-Output "no sub monitor"; exit 1 }
$b = $scr.Bounds
[WinMover]::MoveWindow($proc.MainWindowHandle, $b.X + 60, $b.Y + 40, 1160, 760, $true) | Out-Null
[WinMover]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
Write-Output "moved -> sub at $($b.X + 60),$($b.Y + 40) ($($b.Width)x$($b.Height))"
