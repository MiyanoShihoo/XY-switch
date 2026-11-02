param([Parameter(Mandatory)][string]$Package)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') { throw 'MSI component lookup requires disposable Windows CI' }
$taskInstaller = New-Object -ComObject WindowsInstaller.Installer
$taskDatabase = $taskInstaller.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $taskInstaller, @([IO.Path]::GetFullPath($Package), 0))
function Read-TaskMsiTable([string]$Table) {
  $taskView = $taskDatabase.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $taskDatabase, @('SELECT * FROM `' + $Table + '`'))
  $null = $taskView.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $taskView, $null)
  $taskRows = @()
  try {
    while ($taskRecord = $taskView.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $taskView, $null)) {
      $taskCount = $taskRecord.GetType().InvokeMember('FieldCount', 'GetProperty', $null, $taskRecord, $null)
      $taskFields = for ($taskIndex = 1; $taskIndex -le $taskCount; $taskIndex++) { $taskRecord.GetType().InvokeMember('StringData', 'GetProperty', $null, $taskRecord, @($taskIndex)) }
      $taskRows += ,$taskFields
    }
  } finally { $null = $taskView.GetType().InvokeMember('Close', 'InvokeMethod', $null, $taskView, $null) }
  return ,$taskRows
}
$taskProperties = Read-TaskMsiTable 'Property'
$taskProduct = @($taskProperties | Where-Object { $_[0] -eq 'ProductCode' })
$taskFiles = @(Read-TaskMsiTable 'File' | Where-Object { ($_[2] -split '\|')[-1] -eq 'xy-switch.exe' })
if ($taskProduct.Count -ne 1 -or $taskFiles.Count -ne 1) { throw 'MSI application identity is ambiguous' }
$taskComponent = @(Read-TaskMsiTable 'Component' | Where-Object { $_[0] -eq $taskFiles[0][1] })
if ($taskComponent.Count -ne 1) { throw 'MSI application component is ambiguous' }
$taskProductCode = $taskProduct[0][1]
$taskComponentId = $taskComponent[0][1]
$taskInstalledPath = $taskInstaller.GetType().InvokeMember('ComponentPath', 'GetProperty', $null, $taskInstaller, @($taskProductCode, $taskComponentId))
[ordered]@{ productCode = $taskProductCode; componentId = $taskComponentId; installedPath = $taskInstalledPath } | ConvertTo-Json -Compress
