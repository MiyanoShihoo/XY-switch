param(
    [Parameter(Mandatory = $true)][string]$BusinessInput,
    [Parameter(Mandatory = $true)][string]$CaseRoot
)
$ErrorActionPreference = 'Stop'
$taskInput = Get-Content -LiteralPath $BusinessInput -Raw | ConvertFrom-Json
$taskPackage = @($taskInput.packages | Where-Object { $_.format -eq 'nsis' })
if ($taskPackage.Count -ne 1) { throw 'Expected one NSIS package' }
$taskPackage = $taskPackage[0]
$taskScope = Get-Content -LiteralPath (Join-Path $CaseRoot 'scope.md') -Raw
if (-not $taskScope.Contains($taskPackage.path)) { throw 'Sample is outside granted scope' }
$taskImage = [IO.File]::ReadAllBytes($taskPackage.path)
$taskHash = (Get-FileHash -LiteralPath $taskPackage.path -Algorithm SHA256).Hash.ToLowerInvariant()
if ($taskHash -ne $taskPackage.sha256) { throw 'Package SHA256 mismatch' }
$taskPeOffset = [BitConverter]::ToUInt32($taskImage, 60)
$taskMachine = [BitConverter]::ToUInt16($taskImage, $taskPeOffset + 4)
$taskSectionCount = [BitConverter]::ToUInt16($taskImage, $taskPeOffset + 6)
$taskOptionalSize = [BitConverter]::ToUInt16($taskImage, $taskPeOffset + 20)
$taskOptionalMagic = [BitConverter]::ToUInt16($taskImage, $taskPeOffset + 24)
$taskSectionOffset = $taskPeOffset + 24 + $taskOptionalSize
$taskOverlayOffset = [BitConverter]::ToUInt32($taskImage, $taskPeOffset + 24 + 60)
for ($taskSectionIndex = 0; $taskSectionIndex -lt $taskSectionCount; $taskSectionIndex++) {
    $taskSectionStart = $taskSectionOffset + $taskSectionIndex * 40
    $taskRawSize = [BitConverter]::ToUInt32($taskImage, $taskSectionStart + 16)
    $taskRawOffset = [BitConverter]::ToUInt32($taskImage, $taskSectionStart + 20)
    if ($taskRawSize -gt 0) { $taskOverlayOffset = [Math]::Max($taskOverlayOffset, $taskRawOffset + $taskRawSize) }
}
$taskAscii = [Text.Encoding]::ASCII.GetString($taskImage)
$taskNsisTextOffset = $taskAscii.IndexOf('NullsoftInst', [int]$taskOverlayOffset, [StringComparison]::Ordinal)
if ($taskNsisTextOffset -lt 8) { throw 'NSIS signature text missing in overlay' }
$taskFirstHeaderOffset = $taskNsisTextOffset - 8
$taskSigInfo = [BitConverter]::ToUInt32($taskImage, $taskFirstHeaderOffset + 4)
if ($taskSigInfo -ne 3735928559) { throw 'NSIS signature word differs from DEADBEEF' }
$taskReport = [ordered]@{
    schemaVersion = 1
    tool = 'PowerShell Get-FileHash and .NET BitConverter; independent byte inspection'
    observedAt = [DateTimeOffset]::UtcNow.ToString('o')
    sampleSha256 = $taskHash
    bytes = $taskImage.Length
    peHeaderOffset = $taskPeOffset
    peSignatureHex = [BitConverter]::ToString($taskImage, $taskPeOffset, 4).Replace('-', '').ToLowerInvariant()
    machine = ('0x{0:x4}' -f $taskMachine)
    optionalHeaderMagic = ('0x{0:x4}' -f $taskOptionalMagic)
    sectionCount = $taskSectionCount
    overlayOffset = $taskOverlayOffset
    overlayBytes = $taskImage.Length - $taskOverlayOffset
    nsisFirstHeaderOffset = $taskFirstHeaderOffset
    nsisSignatureWord = ('0x{0:x8}' -f $taskSigInfo)
    nsisLengthOfHeader = [BitConverter]::ToUInt32($taskImage, $taskFirstHeaderOffset + 20)
    nsisLengthOfAllFollowingData = [BitConverter]::ToUInt32($taskImage, $taskFirstHeaderOffset + 24)
    firstHeaderHex = [BitConverter]::ToString($taskImage, $taskFirstHeaderOffset, 28).Replace('-', '').ToLowerInvariant()
    sampleExecuted = $false
    chargedApiCalls = 0
}
$taskOutput = Join-Path $CaseRoot 'evidence\independent-header.json'
$taskUtf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText($taskOutput, ($taskReport | ConvertTo-Json -Depth 8), $taskUtf8)
$taskReport | ConvertTo-Json -Depth 8

