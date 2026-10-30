using System;
using System.Runtime.InteropServices;
using System.Text;

public static class XYNativeDialog {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rectangle);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder name, int capacity);
    [DllImport("user32.dll")] private static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] private static extern bool IsChild(IntPtr parent, IntPtr window);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true, EntryPoint = "SendMessageTimeoutW")]
    private static extern IntPtr SetTextMessage(IntPtr window, uint message, UIntPtr parameter, string text, uint flags, uint timeoutMs, out UIntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true, EntryPoint = "SendMessageTimeoutW")]
    private static extern IntPtr GetTextMessage(IntPtr window, uint message, UIntPtr capacity, StringBuilder text, uint flags, uint timeoutMs, out UIntPtr result);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool PostMessage(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    private static uint ProcessId(IntPtr window) {
        uint processId;
        GetWindowThreadProcessId(window, out processId);
        return processId;
    }
    private static string ClassName(IntPtr window) {
        var name = new StringBuilder(256);
        return GetClassName(window, name, name.Capacity) > 0 ? name.ToString() : "";
    }
    public static bool IsControl(IntPtr dialog, IntPtr control, uint processId, string className, int controlId) {
        return dialog != IntPtr.Zero && control != IntPtr.Zero && processId > 0
            && ProcessId(dialog) == processId && ClassName(dialog) == "#32770"
            && IsChild(dialog, control) && ProcessId(control) == processId
            && ClassName(control) == className && GetDlgCtrlID(control) == controlId
            && IsWindowVisible(dialog) && IsWindowVisible(control);
    }
    private static void RequireControl(IntPtr dialog, IntPtr control, uint processId, string className, int controlId) {
        if (!IsControl(dialog, control, processId, className, controlId) || !IsWindowEnabled(control))
            throw new InvalidOperationException("Native control identity, ancestry or enabled-state mismatch");
    }
    public static void SetFileName(IntPtr dialog, IntPtr edit, uint processId, string path) {
        RequireControl(dialog, edit, processId, "Edit", 1001);
        if (String.IsNullOrEmpty(path) || path.Length >= 32767 || path.IndexOf('\0') >= 0 || !System.IO.Path.IsPathRooted(path))
            throw new ArgumentException("An absolute native fixture path is required");
        UIntPtr result;
        if (SetTextMessage(edit, 0x000C, UIntPtr.Zero, path, 2, 5000, out result) == IntPtr.Zero || result == UIntPtr.Zero)
            throw new InvalidOperationException("Native WM_SETTEXT failed or timed out");
    }
    public static string ReadFileName(IntPtr dialog, IntPtr edit, uint processId) {
        RequireControl(dialog, edit, processId, "Edit", 1001);
        var text = new StringBuilder(32768);
        UIntPtr result;
        if (GetTextMessage(edit, 0x000D, new UIntPtr((uint)text.Capacity), text, 2, 5000, out result) == IntPtr.Zero)
            throw new InvalidOperationException("Native WM_GETTEXT failed or timed out");
        return text.ToString();
    }
    public static void ClickAction(IntPtr dialog, IntPtr button, uint processId, int controlId) {
        if (controlId != 1 && controlId != 2) throw new ArgumentException("Only native file-dialog action buttons are accepted");
        RequireControl(dialog, button, processId, "Button", controlId);
        if (!PostMessage(button, 0x00F5, UIntPtr.Zero, IntPtr.Zero))
            throw new InvalidOperationException("Native BM_CLICK could not be queued");
    }
}
