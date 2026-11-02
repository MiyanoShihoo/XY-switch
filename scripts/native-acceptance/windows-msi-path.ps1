param([Parameter(Mandatory)][string]$Package)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') { throw 'MSI component lookup requires disposable Windows CI' }
$taskInstaller = New-Object -ComObject WindowsInstaller.Installer
$taskDatabase = $taskInstaller.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $taskInstaller, @([IO.Path]::GetFullPath($Package), 0))
. (Join-Path $PSScriptRoot 'windows-msi-identity.ps1')
$taskIdentity = Get-TaskMsiApplicationIdentity $taskDatabase
$taskProductCode = $taskIdentity.productCode
$taskComponentId = $taskIdentity.componentId
$taskInstalledPath = $taskInstaller.GetType().InvokeMember('ComponentPath', 'GetProperty', $null, $taskInstaller, @($taskProductCode, $taskComponentId))
[ordered]@{ productCode = $taskProductCode; componentId = $taskComponentId; installedPath = $taskInstalledPath } | ConvertTo-Json -Compress
