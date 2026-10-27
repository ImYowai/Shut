param([int]$ProcId)
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WinEnum {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    public static List<string> Collect(uint target) {
        var list = new List<string>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != target) return true;
            var t = new StringBuilder(256); GetWindowText(h, t, 256);
            var c = new StringBuilder(256); GetClassName(h, c, 256);
            RECT r; GetWindowRect(h, out r);
            list.Add(string.Format("h={0} vis={1} cls={2} title='{3}' rect={4},{5} {6}x{7}",
                h, IsWindowVisible(h), c, t, r.L, r.T, r.R - r.L, r.B - r.T));
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
public struct RECT { public int L; public int T; public int R; public int B; }
"@
[WinEnum]::Collect($ProcId) | ForEach-Object { $_ }
