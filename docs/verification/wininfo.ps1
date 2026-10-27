param([int]$ProcId = 0)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinInfo {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
}
public struct RECT { public int L; public int T; public int R; public int B; }
"@
Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -or $_.ProcessName -eq "shut-remote" } |
    Where-Object { $ProcId -eq 0 -or $_.Id -eq $ProcId } |
    ForEach-Object {
        $h = $_.MainWindowHandle
        $r = New-Object RECT
        [WinInfo]::GetWindowRect($h, [ref]$r) | Out-Null
        [pscustomobject]@{
            Pid = $_.Id; Name = $_.ProcessName; Title = $_.MainWindowTitle
            Visible = [WinInfo]::IsWindowVisible($h)
            X = $r.L; Y = $r.T; W = ($r.R - $r.L); H = ($r.B - $r.T)
        }
    } | Format-Table -AutoSize
