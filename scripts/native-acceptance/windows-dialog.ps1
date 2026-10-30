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
function Save-DialogScreenshot($Element) {
    $handle = [IntPtr]$Element.Current.NativeWindowHandle
    $rect = New-Object XYDialogCapture+RECT
    if (-not [XYDialogCapture]::GetWindowRect($handle, [ref]$rect)) { return $false }
    if ($rect.Right -le $rect.Left -or $rect.Bottom -le $rect.Top) { return $false }
    $bitmap = New-Object Drawing.Bitmap(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top))
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $dc = $graphics.GetHdc()
        try { $captured = [XYDialogCapture]::PrintWindow($handle, $dc, 2) } finally { $graphics.ReleaseHdc($dc) }
        if ($captured) { $bitmap.Save($OutputPrefix + '.png', [Drawing.Imaging.ImageFormat]::Png) }
        return $captured
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
$uia = [Windows.Automation.AutomationElement]
$children = [Windows.Automation.TreeScope]::Children
$descendants = [Windows.Automation.TreeScope]::Descendants
$condition = New-Object Windows.Automation.AndCondition(
    (New-Object Windows.Automation.PropertyCondition($uia::ProcessIdProperty, $AppProcessId)),
    (New-Object Windows.Automation.PropertyCondition($uia::ClassNameProperty, '#32770'))
)
$dialog = $null
$buttonId = if ($Operation -eq 'cancel') { '2' } else { '1' }
$report = [ordered]@{operation=$Operation;processId=$AppProcessId;completed=$false;screenshotCaptured=$false;controlAttempts=0;nodes=@()}
$started = [DateTime]::UtcNow
$deadline = $started.AddSeconds(75)
$ready = $false
try {
    if ($Operation -ne 'cancel' -and -not [IO.Path]::IsPathRooted($SelectedPath)) { throw 'Native dialog requires an absolute fixture path' }
    # A #32770 window can appear before the shell adds its filename and action controls.
    while ([DateTime]::UtcNow -lt $deadline) {
        if (-not (Get-Process -Id $AppProcessId -ErrorAction SilentlyContinue)) { throw 'Tested application exited before its native dialog was ready' }
        $dialog = $uia::RootElement.FindFirst($children, $condition)
        if ($dialog) {
            try {
                $all = @($dialog.FindAll($descendants, [Windows.Automation.Condition]::TrueCondition))
                $report.controlAttempts++
                $report.processId = $dialog.Current.ProcessId
                $report.class = $dialog.Current.ClassName
                $report.title = $dialog.Current.Name
                $report.nodes = @($all | ForEach-Object { [ordered]@{name=$_.Current.Name;id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName;class=$_.Current.ClassName;enabled=$_.Current.IsEnabled} })
                $edits = @($all | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and ($_.Current.AutomationId -eq '1001' -or $_.Current.Name -match '^(File name:|\u6587\u4ef6\u540d.*)$') })
                $buttons = @($all | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.AutomationId -eq $buttonId })
                $editPattern = $null
                $buttonPattern = $null
                $editReady = $Operation -eq 'cancel' -or ($edits.Count -eq 1 -and $edits[0].Current.IsEnabled -and $edits[0].TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$editPattern))
                $buttonReady = $buttons.Count -eq 1 -and $buttons[0].TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$buttonPattern)
                if ($editReady -and $buttonReady) { $ready = $true; break }
            } catch [Windows.Automation.ElementNotAvailableException] {
                # Re-read a shell subtree replaced while the native dialog initializes.
            }
        }
        Start-Sleep -Milliseconds 150
    }
    $report.controlsWaitMs = [int]([DateTime]::UtcNow - $started).TotalMilliseconds
    if (-not $ready) { throw 'Native filename/action controls did not become ready in the tested process dialog' }
    if ($Operation -ne 'cancel') {
        ([Windows.Automation.ValuePattern]$editPattern).SetValue($SelectedPath)
        $report.fileName = [IO.Path]::GetFileName($SelectedPath)
        $report.editId = $edits[0].Current.AutomationId
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not $buttons[0].Current.IsEnabled -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (-not $buttons[0].Current.IsEnabled) { throw 'Native file dialog action is disabled' }
    $report.screenshotCaptured = Save-DialogScreenshot $dialog
    $report.buttonId = $buttonId
    ([Windows.Automation.InvokePattern]$buttonPattern).Invoke()
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 150
        $remaining = $uia::RootElement.FindFirst($children, $condition)
    } while ($remaining -and [DateTime]::UtcNow -lt $deadline)
    if ($remaining) { throw 'Native file dialog did not close after the requested operation' }
    $report.completed = $true
} catch {
    $report.error = $_.Exception.Message
    if ($dialog -and -not $report.screenshotCaptured) {
        try { $report.screenshotCaptured = Save-DialogScreenshot $dialog }
        catch { $report.screenshotError = $_.Exception.Message }
    }
    throw
} finally {
    $report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath ($OutputPrefix + '.json') -Encoding UTF8
}
