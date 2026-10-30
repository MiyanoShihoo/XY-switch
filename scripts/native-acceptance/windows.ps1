param(
    [Parameter(Mandatory)][string]$ArtifactDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
    throw 'Packaged native sync acceptance requires a disposable GitHub-hosted Windows runner'
}
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location -LiteralPath $root
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$packageJson = & node scripts/native-acceptance/package-input.mjs $ArtifactDirectory windows-x64
if ($LASTEXITCODE -ne 0) { throw 'Source installer verification failed' }
$packageJson | Set-Content -LiteralPath (Join-Path $OutputDirectory 'package-input.json') -Encoding utf8
$package = $packageJson | ConvertFrom-Json
$config = Get-Content -LiteralPath 'src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json
$database = Join-Path ([Environment]::GetFolderPath('ApplicationData')) ($config.identifier + '\xy-switch.db')
if (Test-Path -LiteralPath $database) { throw 'Refusing to use pre-existing application data' }
$temporary = [IO.Path]::GetFullPath((Join-Path $env:RUNNER_TEMP ('xy-sync-native-' + [guid]::NewGuid().ToString('N'))))
$parent = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $temporary.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase)) { throw 'Test directory escaped RUNNER_TEMP' }
New-Item -ItemType Directory -Path $temporary | Out-Null
$install = Join-Path $temporary 'application'
$exe = Join-Path $install 'xy-switch.exe'
$app = $null; $installed = $false; $primaryFailure = $null
$debugPolicyPath = 'HKLM:\Software\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
$hostElevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $hostElevated) { throw 'App-scoped HKLM test policy requires the elevated disposable runner' }
$debugPolicyExisted = Test-Path -LiteralPath $debugPolicyPath
$debugPolicyBackups = [Collections.Generic.List[object]]::new()
$report = [ordered]@{passed=$false;sourceCommit=$env:NATIVE_SOURCE_COMMIT;harnessCommit=$env:GITHUB_SHA;sourceRun=$env:NATIVE_SOURCE_RUN;installed=$false;uninstalled=$false;scope='Packaged Windows NSIS payload, actual WebView DOM, native picker and production Rust IPC; no mock IPC or picker bypass'}
function Invoke-Installer([string]$File, [string[]]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(240000)) { $process.Kill($true); throw 'Installer timed out' }
    if ($process.ExitCode -notin 0,3010) { throw "Installer exited $($process.ExitCode)" }
}
try {
    $report.hostElevated = $hostElevated
    $report.debugPolicyHive = 'HKLM'
    $report.phase = 'installation'
    Invoke-Installer $package.installer @('/S', "/D=$install")
    $installed = $true; $report.installed = $true
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Installer did not create the expected application' }
    $digest = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($digest -ne $package.installedSha256) { throw 'Installed executable differs from the source CI payload' }
    $report.installedSha256 = $digest
    $report.phase = 'webview-debug-listener'
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $exe; $start.WorkingDirectory = $install
    $start.UseShellExecute = $false; $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    # WebView2 150+ ignores environment/HKCU overrides in elevated hosts. Microsoft's
    # supported HKLM override remains app-scoped, with every original value restored.
    $debugArguments = "--remote-debugging-port=$port --remote-debugging-address=127.0.0.1"
    if (-not $debugPolicyExisted) { New-Item -Path $debugPolicyPath -Force | Out-Null }
    foreach ($name in @($config.identifier, 'xy-switch.exe')) {
        $key = Get-Item -LiteralPath $debugPolicyPath
        try {
            $exists = $key.GetValueNames() -contains $name
            $backup = @{name=$name;existed=$exists}
            if ($exists) {
                $backup.value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                $backup.kind = $key.GetValueKind($name)
            }
            $debugPolicyBackups.Add($backup)
        } finally { $key.Dispose() }
        New-ItemProperty -LiteralPath $debugPolicyPath -Name $name -Value $debugArguments -PropertyType String -Force | Out-Null
    }
    $report.debugPolicyScope = @($debugPolicyBackups | ForEach-Object { @{name=$_.name;preExisting=$_.existed} })
    # Never apply a wildcard policy or change the executable/package.
    $start.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = $debugArguments
    $start.Environment['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $temporary 'webview'
    $report.testWebViewArguments = $start.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']
    $app = [Diagnostics.Process]::Start($start)
    $report.appProcessId = $app.Id
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    do {
        if ($app.HasExited) { throw 'Packaged application exited before its WebView was ready' }
        $connections = @(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue)
        if ($connections.Count) { break }
        Start-Sleep -Milliseconds 500
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $connections.Count) { throw 'Test-only WebView CDP did not start' }
    if (@($connections | Where-Object LocalAddress -notin '127.0.0.1','::1').Count) { throw 'CDP must not listen outside loopback' }
    $report.debugListenerAddresses = @($connections.LocalAddress)
    $runtime = Invoke-RestMethod -Uri "http://127.0.0.1:$port/json/version" -TimeoutSec 10
    $report.webviewRuntime = $runtime.Browser
    $report.debugProtocolVersion = $runtime.'Protocol-Version'
    $report.phase = 'native-ui'
    & node scripts/native-acceptance/windows-sync.mjs --endpoint "http://127.0.0.1:$port" --pid $app.Id --output $OutputDirectory --files $temporary --database $database
    if ($LASTEXITCODE -ne 0) { throw 'Native encrypted sync acceptance failed; see results.json' }
    $app.Kill($true); $app.WaitForExit(15000) | Out-Null
    $report.phase = 'database-integrity'
    & python -c 'import json,pathlib,sqlite3,sys; c=sqlite3.connect(pathlib.Path(sys.argv[1]).resolve().as_uri()+"?mode=ro",uri=True); r=c.execute("PRAGMA integrity_check").fetchone()[0]; assert r=="ok"; f=c.execute("PRAGMA foreign_key_check").fetchall(); assert not f; print(json.dumps({"integrity":r,"foreignKeyViolations":len(f)})); c.close()' $database | Set-Content -LiteralPath (Join-Path $OutputDirectory 'database-integrity.json') -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw 'Native SQLite integrity check failed' }
    $report.passed = $true
} catch {
    $primaryFailure = $_
    $report.error = $_.ToString()
    try {
        $candidates = @(@($temporary, [Environment]::GetFolderPath('MyDocuments')) | Select-Object -Unique | ForEach-Object {
            if (Test-Path -LiteralPath $_ -PathType Container) {
                Get-ChildItem -LiteralPath $_ -Filter '*.xysync' -File | ForEach-Object {
                    [ordered]@{path=$_.FullName;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
                }
            }
        })
        ConvertTo-Json -InputObject $candidates -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'native-file-location-diagnostics.json') -Encoding utf8
    } catch { $report.fileDiagnosticError = $_.ToString() }
    if ($app) {
        try { & (Join-Path $PSScriptRoot 'windows-diagnostics.ps1') -AppProcessId $app.Id -OutputDirectory $OutputDirectory }
        catch { $report.diagnosticError = $_.ToString() }
    }
} finally {
    try {
        if ($app -and -not $app.HasExited) { $app.Kill($true); $app.WaitForExit(15000) | Out-Null }
        if ($installed) {
            Invoke-Installer (Join-Path $install 'uninstall.exe') @('/S')
            # NSIS detaches a temporary uninstaller; its launcher can exit before deletion finishes.
            for ($attempt = 0; $attempt -lt 30 -and (Test-Path -LiteralPath $exe); $attempt++) { Start-Sleep -Seconds 2 }
            if (Test-Path -LiteralPath $exe) { throw 'Native test uninstallation left its executable' }
            $report.uninstalled = $true
        }
    } catch {
        $report.cleanupError = $_.ToString()
        if (-not $primaryFailure) { $primaryFailure = $_ }
    } finally {
        try {
            foreach ($backup in $debugPolicyBackups) {
                if ($backup.existed) {
                    $key = Get-Item -LiteralPath $debugPolicyPath
                    try { $key.SetValue($backup.name, $backup.value, $backup.kind) } finally { $key.Dispose() }
                } else {
                    Remove-ItemProperty -LiteralPath $debugPolicyPath -Name $backup.name -ErrorAction Stop
                }
            }
            if (-not $debugPolicyExisted -and (Test-Path -LiteralPath $debugPolicyPath)) {
                $key = Get-Item -LiteralPath $debugPolicyPath
                try { $empty = $key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0 } finally { $key.Dispose() }
                if ($empty) { Remove-Item -LiteralPath $debugPolicyPath -ErrorAction Stop }
            }
            $report.debugPolicyRestored = $true
        } catch {
            $report.debugPolicyCleanupError = $_.ToString()
            if (-not $primaryFailure) { $primaryFailure = $_ }
        }
        $report.passed = $report.passed -and $report.uninstalled -and $report.debugPolicyRestored
        $report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'native-installation.json') -Encoding utf8
    }
}

if ($primaryFailure) { throw $primaryFailure }
