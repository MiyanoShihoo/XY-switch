param([Parameter(Mandatory)][int]$AppProcessId,[Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { throw 'Native diagnostics require disposable GitHub-hosted CI' }
$rows=@(Get-CimInstance Win32_Process)
$ids=[Collections.Generic.HashSet[int]]::new();$null=$ids.Add($AppProcessId)
do {
  $added=$false
  foreach($row in $rows) { if($ids.Contains([int]$row.ParentProcessId) -and $ids.Add([int]$row.ProcessId)) { $added=$true } }
} while($added)
$report=[ordered]@{processes=@($rows|Where-Object {$ids.Contains([int]$_.ProcessId)}|Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine);listeners=@()}
try { $report.listeners=@(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object {$ids.Contains([int]$_.OwningProcess)} | Select-Object LocalAddress,LocalPort,OwningProcess) } catch { $report.listenerError=$_.ToString() }
try {
  $app=Get-Process -Id $AppProcessId -ErrorAction Stop;$app.Refresh()
  $report.mainWindowHandle=$app.MainWindowHandle.ToInt64();$report.mainWindowTitle=$app.MainWindowTitle
  if($app.MainWindowHandle -ne [IntPtr]::Zero) {
    Add-Type -AssemblyName System.Drawing
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class XYNativeWindowCapture {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out RECT rectangle);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
"@
    $rect=[XYNativeWindowCapture+RECT]::new()
    if([XYNativeWindowCapture]::GetWindowRect($app.MainWindowHandle,[ref]$rect)) {
      $width=$rect.Right-$rect.Left;$height=$rect.Bottom-$rect.Top
      if($width -gt 0 -and $height -gt 0 -and $width -lt 8192 -and $height -lt 8192) {
        $bitmap=[Drawing.Bitmap]::new($width,$height);$graphics=[Drawing.Graphics]::FromImage($bitmap)
        try {
          $dc=$graphics.GetHdc()
          try { $report.screenshotCaptured=[XYNativeWindowCapture]::PrintWindow($app.MainWindowHandle,$dc,2) } finally {$graphics.ReleaseHdc($dc)}
          if($report.screenshotCaptured) {$bitmap.Save((Join-Path $OutputDirectory 'startup-failure.png'),[Drawing.Imaging.ImageFormat]::Png)}
        } finally {$graphics.Dispose();$bitmap.Dispose()}
      }
    }
  }
} catch {$report.windowError=$_.ToString()}
$report|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $OutputDirectory 'native-process-diagnostics.json') -Encoding utf8
