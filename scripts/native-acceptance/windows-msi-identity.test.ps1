param([Parameter(Mandatory)][string]$Temporary)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-msi-identity.ps1')
$taskPackage = [IO.Path]::GetFullPath((Join-Path $Temporary 'identity-fixture.msi'))
$taskInstaller = New-Object -ComObject WindowsInstaller.Installer
$taskDatabase = $taskInstaller.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $taskInstaller, @($taskPackage, [int16]3))
function Invoke-TaskFixtureQuery([string]$Query) {
  $taskView = $taskDatabase.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $taskDatabase, @($Query))
  try { $null = $taskView.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $taskView, $null) }
  finally { $null = $taskView.GetType().InvokeMember('Close', 'InvokeMethod', $null, $taskView, $null) }
}
foreach ($taskQuery in @(
  'CREATE TABLE `Property` (`Property` CHAR(72) NOT NULL, `Value` CHAR(255) PRIMARY KEY `Property`)',
  'CREATE TABLE `File` (`File` CHAR(72) NOT NULL, `Component_` CHAR(72), `FileName` CHAR(255) PRIMARY KEY `File`)',
  'CREATE TABLE `Component` (`Component` CHAR(72) NOT NULL, `ComponentId` CHAR(38) PRIMARY KEY `Component`)',
  'INSERT INTO `Property` (`Property`, `Value`) VALUES (''ProductName'', ''XY Switch'')',
  'INSERT INTO `Property` (`Property`, `Value`) VALUES (''ProductCode'', ''{D7527AD1-29A1-46D5-ABD5-FDE574C7E625}'')',
  'INSERT INTO `File` (`File`, `Component_`, `FileName`) VALUES (''other'', ''other-component'', ''other.txt'')',
  'INSERT INTO `File` (`File`, `Component_`, `FileName`) VALUES (''application'', ''application-component'', ''XY-SWI~1.EXE|xy-switch.exe'')',
  'INSERT INTO `Component` (`Component`, `ComponentId`) VALUES (''other-component'', ''{11111111-1111-1111-1111-111111111111}'')',
  'INSERT INTO `Component` (`Component`, `ComponentId`) VALUES (''application-component'', ''{5A553FFE-C6EA-556F-81AD-EA86F3651DF1}'')'
)) { Invoke-TaskFixtureQuery $taskQuery }
$taskIdentity = Get-TaskMsiApplicationIdentity $taskDatabase
if ($taskIdentity.productCode -ne '{D7527AD1-29A1-46D5-ABD5-FDE574C7E625}' -or $taskIdentity.componentId -ne '{5A553FFE-C6EA-556F-81AD-EA86F3651DF1}') { throw 'MSI row selection returned incorrect identity' }
Invoke-TaskFixtureQuery 'INSERT INTO `File` (`File`, `Component_`, `FileName`) VALUES (''duplicate'', ''application-component'', ''xy-switch.exe'')'
try { $null = Get-TaskMsiApplicationIdentity $taskDatabase; throw 'Duplicate executable was accepted' }
catch { if ($_.Exception.Message -ne 'MSI application identity is ambiguous') { throw } }
Invoke-TaskFixtureQuery 'DELETE FROM `File`'
try { $null = Get-TaskMsiApplicationIdentity $taskDatabase; throw 'Missing executable was accepted' }
catch { if ($_.Exception.Message -ne 'MSI application identity is ambiguous') { throw } }
[ordered]@{ passed = $true; cases = 3; installed = $false } | ConvertTo-Json -Compress
