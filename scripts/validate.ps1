# Release gate used by the release block: .\scripts\validate.ps1 -Full
# -Full = the whole test suite plus pre-tag verification; without -Full the quick suite.
param([switch]$Full)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
if ($Full) {
  & (Join-Path $PSScriptRoot "test.ps1")
  & (Join-Path $PSScriptRoot "verify-release.ps1") -PreTag
} else {
  & (Join-Path $PSScriptRoot "test.ps1") -Quick
}
Write-Host "validate: ok" -ForegroundColor Green
