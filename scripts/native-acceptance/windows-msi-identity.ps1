function Read-TaskMsiRows([object]$Database, [string]$Table) {
  if ($Table -notin @('Property', 'File', 'Component')) { throw 'Unexpected MSI identity table' }
  $taskView = $Database.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $Database, @('SELECT * FROM `' + $Table + '`'))
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
function Get-TaskMsiApplicationIdentity([object]$Database) {
  $taskProperties = Read-TaskMsiRows $Database 'Property'
  $taskFileRows = Read-TaskMsiRows $Database 'File'
  $taskComponentRows = Read-TaskMsiRows $Database 'Component'
  $taskProduct = @($taskProperties | Where-Object { $_[0] -eq 'ProductCode' })
  $taskFiles = @($taskFileRows | Where-Object { ($_[2] -split '\|')[-1] -eq 'xy-switch.exe' })
  if ($taskProduct.Count -ne 1 -or $taskFiles.Count -ne 1) { throw 'MSI application identity is ambiguous' }
  $taskComponent = @($taskComponentRows | Where-Object { $_[0] -eq $taskFiles[0][1] })
  if ($taskComponent.Count -ne 1) { throw 'MSI application component is ambiguous' }
  [pscustomobject][ordered]@{ productCode = $taskProduct[0][1]; componentId = $taskComponent[0][1] }
}
