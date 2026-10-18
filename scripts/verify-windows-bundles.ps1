$ErrorActionPreference = 'Stop'
# Release builds deliberately ignore development data overrides. Never run this on an operator's account.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'Installer/startup verification is restricted to a disposable GitHub-hosted Windows runner.'
}
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
$config = Get-Content -LiteralPath (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$version = $config.version
$output = Join-Path $root "release\$version\windows-x64"
$manifest = Get-Content -LiteralPath (Join-Path $output 'manifest.json') -Raw | ConvertFrom-Json
foreach ($entry in $manifest.files) {
    if ([IO.Path]::GetFileName($entry.name) -ne $entry.name) { throw 'Unsafe package filename' }
    $file = Join-Path $output $entry.name
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256 -or (Get-Item -LiteralPath $file).Length -ne $entry.bytes) { throw 'Final Windows package checksum mismatch' }
}
$temporary = Join-Path $env:RUNNER_TEMP ('xy-switch-windows-' + [guid]::NewGuid().ToString('N'))
$resolved = [IO.Path]::GetFullPath($temporary)
$parent = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase)) { throw 'Temporary installation escapes the runner directory' }
New-Item -ItemType Directory -Path $temporary | Out-Null
$database = Join-Path ([Environment]::GetFolderPath('ApplicationData')) ($config.identifier + '\xy-switch.db')
if (Test-Path -LiteralPath $database) { throw 'Refusing to test against pre-existing application data' }
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class XYWindowCapture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rectangle);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
"@
function Capture-Window([IntPtr]$handle, [string]$file) {
    $rectangle = [XYWindowCapture+RECT]::new()
    if (-not [XYWindowCapture]::GetWindowRect($handle, [ref]$rectangle)) { return $false }
    $width = $rectangle.Right - $rectangle.Left; $height = $rectangle.Bottom - $rectangle.Top
    if ($width -lt 600 -or $height -lt 400 -or $width -gt 8192 -or $height -gt 8192) { return $false }
    $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $dc = $graphics.GetHdc()
        try { $captured = [XYWindowCapture]::PrintWindow($handle, $dc, 2) } finally { $graphics.ReleaseHdc($dc) }
        if (-not $captured) { return $false }
        $bitmap.Save($file, [Drawing.Imaging.ImageFormat]::Png)
        return $true
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
function Invoke-Installer([string]$file, [string[]]$arguments) {
    $process = Start-Process -FilePath $file -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(240000)) { throw 'Installer exceeded four minutes' }
    if ($process.ExitCode -notin 0,3010) { throw "Installer failed with exit code $($process.ExitCode)" }
    return $process.ExitCode
}
$results = @()
foreach ($format in 'nsis','msi') {
    $package = @($manifest.files | Where-Object format -eq $format)
    if ($package.Count -ne 1) { throw "Expected one $format package" }
    $installer = Join-Path $output $package[0].name
    $install = Join-Path $temporary $format
    $installedExe = Join-Path $install 'xy-switch.exe'
    $app = $null; $installed = $false
    $result = [ordered]@{format=$format;passed=$false;installation=$false;uninstallation=$false;scope='Final installer installation, native window, isolated CI data and uninstall; not all Windows versions'}
    try {
        if ($format -eq 'nsis') { $null = Invoke-Installer $installer @('/S',"/D=$install") }
        else { $null = Invoke-Installer 'msiexec.exe' @('/i',"`"$installer`"",'/qn','/norestart',"INSTALLDIR=`"$install`"",'/l*v',"`"$(Join-Path $output 'msi-install.log')`"") }
        $installed = $true
        if (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) { throw 'Installer did not place the expected application in its test directory' }
        $built = Join-Path $root 'src-tauri\target\release\xy-switch.exe'
        $payloadJson = & node (Join-Path $root 'scripts/verify-windows-payload.mjs') $built $installedExe $format
        if ($LASTEXITCODE -ne 0) { throw 'Installed application differs from the newly built packaging input' }
        $payloadJson | Set-Content -LiteralPath (Join-Path $output "native-$format-payload-check.json") -Encoding utf8
        $payload = $payloadJson | ConvertFrom-Json
        $result.installation = $true
        $result.installedBinarySha256 = $payload.installedSha256
        $result.matchesPackagingInputExceptBundleMarker = $payload.matchesPackagingInputExceptBundleMarker
        $result.authenticode = (Get-AuthenticodeSignature -FilePath $installer).Status.ToString()
        $app = Start-Process -FilePath $installedExe -WorkingDirectory $install -WindowStyle Hidden -PassThru
        $rendered = $false
        for ($attempt=0; $attempt -lt 30; $attempt++) {
            Start-Sleep -Seconds 3
            $app.Refresh()
            if ($app.HasExited) { throw 'Installed application exited before rendering' }
            if ($app.MainWindowHandle -eq [IntPtr]::Zero) { continue }
            $image = Join-Path $output "native-$format-launch.png"
            if (-not (Capture-Window $app.MainWindowHandle $image)) { continue }
            $check = & node (Join-Path $root 'scripts\verify-native-screenshot.mjs') $image
            $check | Set-Content -LiteralPath (Join-Path $output "native-$format-screenshot-check.json") -Encoding utf8
            if ($LASTEXITCODE -eq 0) { $rendered=$true; break }
        }
        if (-not $rendered) { throw 'Installed application did not produce a nonblank native window' }
        if (-not (Test-Path -LiteralPath $database -PathType Leaf)) { throw 'Expected fresh CI application database was not created' }
        & python -c 'import pathlib,sqlite3,sys;c=sqlite3.connect(pathlib.Path(sys.argv[1]).resolve().as_uri()+"?mode=ro",uri=True);assert c.execute("PRAGMA integrity_check").fetchone()[0]=="ok";c.close()' $database
        if ($LASTEXITCODE -ne 0) { throw 'CI application database integrity check failed' }
        $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object OwningProcess -eq $app.Id)
        if ($listeners.Count -ne 0) { throw 'Unexpected TCP listener on startup' }
        $result.pid = $app.Id; $result.rendered = $true; $result.databaseIntegrity = 'ok'; $result.tcpListeners=0
    } finally {
        if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id; $app.WaitForExit(15000) | Out-Null }
        if ($installed) {
            if ($format -eq 'nsis') { $null = Invoke-Installer (Join-Path $install 'uninstall.exe') @('/S') }
            else { $null = Invoke-Installer 'msiexec.exe' @('/x',"`"$installer`"",'/qn','/norestart','/l*v',"`"$(Join-Path $output 'msi-uninstall.log')`"") }
            for ($attempt=0; $attempt -lt 30 -and (Test-Path -LiteralPath $installedExe); $attempt++) { Start-Sleep -Seconds 2 }
            $result.uninstallation = -not (Test-Path -LiteralPath $installedExe)
        }
        $result.passed = [bool]($result.installation -and $result.rendered -and $result.uninstallation)
        $results += [pscustomobject]$result
        $results | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'native-windows-verification.json') -Encoding utf8
    }
    if (-not $result.passed) { throw "Final $format package verification failed" }
}
$results | ConvertTo-Json -Depth 8
