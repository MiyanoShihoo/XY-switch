param([switch]$Release)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

function Invoke-Checked([string]$Command, [string[]]$Arguments) {
  & $Command @Arguments
  if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
}

Invoke-Checked "pnpm" @("install", "--frozen-lockfile")
Invoke-Checked "pnpm" @("build")
Invoke-Checked "cargo" @("check", "--manifest-path", "src-tauri\Cargo.toml")
if ($Release) {
  Invoke-Checked "pnpm" @("tauri", "build", "--bundles", "msi")
}
