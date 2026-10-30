param(
    [Parameter(Mandatory)][int]$AppProcessId,
    [Parameter(Mandatory)][ValidateSet('save','open','cancel')][string]$Operation,
    [string]$SelectedPath,
    [Parameter(Mandatory)][string]$OutputPrefix
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Native dialog automation requires disposable GitHub-hosted CI' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class XYDialogCapture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rectangle);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
"@
$uia = [Windows.Automation.AutomationElement]
$children = [Windows.Automation.TreeScope]::Children
$descendants = [Windows.Automation.TreeScope]::Descendants
$condition = New-Object Windows.Automation.AndCondition(
    (New-Object Windows.Automation.PropertyCondition($uia::ProcessIdProperty, $AppProcessId)),
    (New-Object Windows.Automation.PropertyCondition($uia::ClassNameProperty, '#32770'))
)
$dialog = $null
$deadline = [DateTime]::UtcNow.AddSeconds(75)
while ([DateTime]::UtcNow -lt $deadline) {
    if (-not (Get-Process -Id $AppProcessId -ErrorAction SilentlyContinue)) { throw 'Tested application exited before its native dialog appeared' }
    $dialog = $uia::RootElement.FindFirst($children, $condition)
    if ($dialog) { break }
    Start-Sleep -Milliseconds 150
}
if (-not $dialog) { throw 'No native #32770 dialog belonging to the tested process appeared' }
$all = @($dialog.FindAll($descendants, [Windows.Automation.Condition]::TrueCondition))
$nodes = @($all | ForEach-Object { [ordered]@{name=$_.Current.Name;id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName;class=$_.Current.ClassName} })
$report = [ordered]@{operation=$Operation;processId=$dialog.Current.ProcessId;class=$dialog.Current.ClassName;title=$dialog.Current.Name;nodes=$nodes;completed=$false}
try {
    if ($Operation -ne 'cancel') {
        if (-not [IO.Path]::IsPathRooted($SelectedPath)) { throw 'Native dialog requires an absolute fixture path' }
        $edits = @($all | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and ($_.Current.AutomationId -eq '1001' -or $_.Current.Name -match '^(File name:|文件名.*)$') })
        if ($edits.Count -ne 1) { throw "Expected one native filename edit control; found $($edits.Count)" }
        $edit = $edits[0]
        ([Windows.Automation.ValuePattern]$edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)).SetValue($SelectedPath)
        $report.fileName = [IO.Path]::GetFileName($SelectedPath)
        $report.editId = $edit.Current.AutomationId
        $buttonId = '1'
    } else { $buttonId = '2' }
    $buttons = @($all | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.AutomationId -eq $buttonId })
    if ($buttons.Count -ne 1) { throw "Expected one native dialog button $buttonId; found $($buttons.Count)" }
    $handle = [IntPtr]$dialog.Current.NativeWindowHandle
    $rect = New-Object XYDialogCapture+RECT
    if ([XYDialogCapture]::GetWindowRect($handle, [ref]$rect)) {
        $bitmap = New-Object Drawing.Bitmap(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top))
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $dc = $graphics.GetHdc()
            try { $captured = [XYDialogCapture]::PrintWindow($handle, $dc, 2) } finally { $graphics.ReleaseHdc($dc) }
            if ($captured) { $bitmap.Save($OutputPrefix + '.png', [Drawing.Imaging.ImageFormat]::Png) }
            $report.screenshotCaptured = $captured
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    }
    $report.buttonId = $buttonId
    ([Windows.Automation.InvokePattern]$buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)).Invoke()
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 150
        $remaining = $uia::RootElement.FindFirst($children, $condition)
    } while ($remaining -and [DateTime]::UtcNow -lt $deadline)
    if ($remaining) { throw 'Native file dialog did not close after the requested operation' }
    $report.completed = $true
} finally {
    $report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath ($OutputPrefix + '.json') -Encoding UTF8
}
