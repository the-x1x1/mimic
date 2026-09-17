# Syntax-checks every Lua file in the plugin and runs the JSON library test.
# Uses lua5.1/luac5.1 when present (CI installs them); otherwise reports SKIPPED.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\lightroom")
$luac = Get-Command luac5.1 -ErrorAction SilentlyContinue
$lua = Get-Command lua5.1 -ErrorAction SilentlyContinue
if (-not $luac) { $luac = Get-Command luac -ErrorAction SilentlyContinue }
if (-not $lua) { $lua = Get-Command lua -ErrorAction SilentlyContinue }
if (-not $luac -or -not $lua) { Write-Host "SKIPPED: lua 5.1 not installed (CI runs this step)" -ForegroundColor Yellow; return }
Get-ChildItem Mimic.lrplugin -Filter *.lua | ForEach-Object {
  & $luac.Source -p $_.FullName
  if ($LASTEXITCODE -ne 0) { throw "Lua syntax error in $($_.Name)" }
}
& $lua.Source tests/json_test.lua
if ($LASTEXITCODE -ne 0) { throw "Json.lua test failed" }
Write-Host "plugin: syntax ok, json_test ok"
