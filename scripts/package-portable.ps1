param([string]$Output = "release\XY-Switch-portable.zip")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$exe = "src-tauri\target\release\xy-switch.exe"
if (!(Test-Path $exe)) { throw "Build the release executable before packaging." }
New-Item -ItemType Directory -Force -Path (Split-Path $Output -Parent) | Out-Null
Compress-Archive -Force -Path $exe -DestinationPath $Output
Write-Output $Output

