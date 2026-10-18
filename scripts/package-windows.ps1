param(
  [string]$ToolsDir = $env:XY_SWITCH_TOOLS_DIR,
  [string]$OutputDir,
  [ValidateSet('nsis,msi', 'nsis', 'msi')][string]$Bundles = 'nsis,msi'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$arguments = @((Join-Path $PSScriptRoot 'package-release.mjs'), 'windows', '--bundles', $Bundles)
if ($ToolsDir) { $arguments += @('--tools-dir', $ToolsDir) }
if ($OutputDir) { $arguments += @('--out-dir', $OutputDir) }
Push-Location -LiteralPath $root
try {
  & node @arguments
  if ($LASTEXITCODE -ne 0) { throw "Full Windows build failed with exit code $LASTEXITCODE" }
} finally { Pop-Location }
