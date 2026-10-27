Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WL {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    public static void List() {
        EnumWindows((h, _) => {
            if (!IsWindowVisible(h)) return true;
            var sb = new StringBuilder(256);
            GetWindowText(h, sb, 256);
            var t = sb.ToString();
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (t.Length > 0) Console.WriteLine("{0} | {1}", pid, t);
            return true;
        }, IntPtr.Zero);
    }
}
"@
[WL]::List()
