param(
    [Parameter(Mandatory)][int]$AppProcessId,
    [Parameter(Mandatory)][ValidateSet('save','open','cancel')][string]$Operation,
    [string]$SelectedPath,
    [Parameter(Mandatory)][string]$OutputPrefix
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Native dialog automation requires disposable GitHub-hosted CI' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'windows-dialog-native.cs')
function Save-DialogScreenshot($Element) {
    $handle = [IntPtr]$Element.Current.NativeWindowHandle
    $rect = New-Object XYNativeDialog+RECT
    if (-not [XYNativeDialog]::GetWindowRect($handle, [ref]$rect)) { return $false }
    if ($rect.Right -le $rect.Left -or $rect.Bottom -le $rect.Top) { return $false }
    $bitmap = New-Object Drawing.Bitmap(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top))
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $dc = $graphics.GetHdc()
        try { $captured = [XYNativeDialog]::PrintWindow($handle, $dc, 2) } finally { $graphics.ReleaseHdc($dc) }
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
                $dialogHandle = [IntPtr]$dialog.Current.NativeWindowHandle
                $report.dialogHandle = $dialogHandle.ToInt64()
                $report.nodes = @($all | ForEach-Object { [ordered]@{name=$_.Current.Name;id=$_.Current.AutomationId;type=$_.Current.ControlType.ProgrammaticName;class=$_.Current.ClassName;enabled=$_.Current.IsEnabled;handle=$_.Current.NativeWindowHandle} })
                # The runner can expose standard Win32 Edit/Button controls as UIA Pane.
                $edits = @($all | Where-Object { $_.Current.ClassName -eq 'Edit' -and $_.Current.AutomationId -eq '1001' })
                $buttons = @($all | Where-Object { $_.Current.ClassName -eq 'Button' -and $_.Current.AutomationId -eq $buttonId })
                $editPattern = $null
                $buttonPattern = $null
                $editReady = $Operation -eq 'cancel'
                if (-not $editReady -and $edits.Count -eq 1) {
                    $editHandle = [IntPtr]$edits[0].Current.NativeWindowHandle
                    $editReady = [XYNativeDialog]::IsControl($dialogHandle, $editHandle, $AppProcessId, 'Edit', 1001) -and [XYNativeDialog]::IsWindowEnabled($editHandle)
                    if ($editReady) { $null = $edits[0].TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$editPattern) }
                }
                $buttonReady = $false
                if ($buttons.Count -eq 1) {
                    $buttonHandle = [IntPtr]$buttons[0].Current.NativeWindowHandle
                    $buttonReady = [XYNativeDialog]::IsControl($dialogHandle, $buttonHandle, $AppProcessId, 'Button', [int]$buttonId)
                    if ($buttonReady) { $null = $buttons[0].TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$buttonPattern) }
                }
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
        if ($editPattern) {
            ([Windows.Automation.ValuePattern]$editPattern).SetValue($SelectedPath)
            $report.editMethod = 'uia-value-pattern'
        } else {
            [XYNativeDialog]::SetFileName($dialogHandle, $editHandle, $AppProcessId, $SelectedPath)
            $report.editMethod = 'win32-em-replacesel'
        }
        if ([XYNativeDialog]::ReadFileName($dialogHandle, $editHandle, $AppProcessId) -cne $SelectedPath) { throw 'Native filename edit did not retain the exact selected path' }
        $report.requestedPath = $SelectedPath
        $report.fileName = [IO.Path]::GetFileName($SelectedPath)
        $report.editId = $edits[0].Current.AutomationId
        $report.editHandle = $editHandle.ToInt64()
        $report.editPatterns = @($edits[0].GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
        $report.fileNameVerified = $true
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not [XYNativeDialog]::IsWindowEnabled($buttonHandle) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (-not [XYNativeDialog]::IsControl($dialogHandle, $buttonHandle, $AppProcessId, 'Button', [int]$buttonId) -or -not [XYNativeDialog]::IsWindowEnabled($buttonHandle)) { throw 'Native file dialog action is unavailable' }
    $report.screenshotCaptured = Save-DialogScreenshot $dialog
    $report.buttonId = $buttonId
    $report.buttonHandle = $buttonHandle.ToInt64()
    $report.buttonPatterns = @($buttons[0].GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
    if ($buttonPattern) {
        ([Windows.Automation.InvokePattern]$buttonPattern).Invoke()
        $report.actionMethod = 'uia-invoke-pattern'
    } else {
        [XYNativeDialog]::ClickAction($dialogHandle, $buttonHandle, $AppProcessId, [int]$buttonId)
        $report.actionMethod = 'win32-bm-click'
    }
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
