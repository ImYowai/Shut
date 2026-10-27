param([string]$OutFile, [string]$TitleLike = "Shut Remote")
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public class Win {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    public struct RECT { public int Left, Top, Right, Bottom; }
    public static IntPtr Find(string like) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, _) => {
            var sb = new StringBuilder(256);
            GetWindowText(h, sb, 256);
            if (IsWindowVisible(h) && sb.ToString().Contains(like)) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@
$w = [Win]::Find($TitleLike)
if ($w -eq [IntPtr]::Zero) { Write-Output "window not found: $TitleLike"; exit 1 }
[Win]::ShowWindow($w, 9) | Out-Null
[Win]::SetForegroundWindow($w) | Out-Null
Start-Sleep -Milliseconds 800
$rc = New-Object Win+RECT
[Win]::GetWindowRect($w, [ref]$rc) | Out-Null
$wpx = $rc.Right - $rc.Left; $hpx = $rc.Bottom - $rc.Top
$bmp = New-Object System.Drawing.Bitmap $wpx, $hpx
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($rc.Left, $rc.Top, 0, 0, $bmp.Size)
$bmp.Save($OutFile)
Write-Output "saved $wpx x $hpx -> $OutFile"
