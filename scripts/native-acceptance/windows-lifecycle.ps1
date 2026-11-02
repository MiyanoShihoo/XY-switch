param(
    [Parameter(Mandatory)][string]$ArtifactDirectory,
    [Parameter(Mandatory)][string]$PreviousDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
    throw 'Native lifecycle acceptance requires a disposable GitHub-hosted Windows runner'
}
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location -LiteralPath $root
$config = Get-Content -LiteralPath 'src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json
$appData = [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath('ApplicationData')) $config.identifier))
if (Test-Path -LiteralPath $appData) { throw 'Refusing pre-existing application data' }
$temporary = [IO.Path]::GetFullPath((Join-Path $env:RUNNER_TEMP ('xy-lifecycle-' + [guid]::NewGuid().ToString('N'))))
$parent = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $temporary.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase)) { throw 'Test directory escaped RUNNER_TEMP' }
New-Item -ItemType Directory -Path $temporary | Out-Null
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$policy = 'HKLM:\Software\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $elevated) { throw 'App-scoped test policy requires the disposable elevated runner' }
$existed = Test-Path -LiteralPath $policy
$backups = [Collections.Generic.List[object]]::new()
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$arguments = "--remote-debugging-port=$port --remote-debugging-address=127.0.0.1"
$failure = $null
$report = [ordered]@{passed=$false;policyRestored=$false;sourceCommit=$env:NATIVE_SOURCE_COMMIT;harnessCommit=$env:GITHUB_SHA;sourceRun=$env:NATIVE_SOURCE_RUN;scope='Disposable Windows runner; app-scoped WebView2 diagnostics policy; original policy restored'}
try {
    if (-not $existed) { New-Item -Path $policy -Force | Out-Null }
    foreach ($name in @($config.identifier, 'xy-switch.exe')) {
        $key = Get-Item -LiteralPath $policy
        try {
            $backup = @{name=$name;existed=$key.GetValueNames() -contains $name}
            if ($backup.existed) {
                $backup.value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                $backup.kind = $key.GetValueKind($name)
            }
            $backups.Add($backup)
        } finally { $key.Dispose() }
        New-ItemProperty -LiteralPath $policy -Name $name -Value $arguments -PropertyType String -Force | Out-Null
    }
    & node scripts/native-acceptance/windows-lifecycle.mjs --artifact $ArtifactDirectory --previous $PreviousDirectory --output $OutputDirectory --temporary $temporary --app-data $appData --port $port
    if ($LASTEXITCODE -ne 0) { throw 'Packaged lifecycle acceptance failed; see lifecycle-results.json' }
    $report.passed = $true
} catch { $failure = $_; $report.error = $_.ToString() }
finally {
    try {
        foreach ($backup in $backups) {
            if ($backup.existed) {
                $key = Get-Item -LiteralPath $policy
                try { $key.SetValue($backup.name, $backup.value, $backup.kind) } finally { $key.Dispose() }
            } else { Remove-ItemProperty -LiteralPath $policy -Name $backup.name -ErrorAction Stop }
        }
        if (-not $existed -and (Test-Path -LiteralPath $policy)) {
            $key = Get-Item -LiteralPath $policy
            try { $empty = $key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0 } finally { $key.Dispose() }
            if ($empty) { Remove-Item -LiteralPath $policy -ErrorAction Stop }
        }
        $report.policyRestored = $true
    } catch { $report.policyError = $_.ToString(); if (-not $failure) { $failure = $_ } }
    $report.passed = $report.passed -and $report.policyRestored
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'native-lifecycle-host.json') -Encoding utf8
}
if ($failure) { throw $failure }
