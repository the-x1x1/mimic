# Copies Mimic.lrplugin into the app-managed location Lightroom's Plug-in
# Manager should point at, or verifies the plugin folder is complete (-Verify).
# Never touches Lightroom preferences (spec §49).
param([switch]$Verify, [string]$Destination)
$ErrorActionPreference = "Stop"
$repo = Join-Path $PSScriptRoot ".."
$src = Join-Path $repo "lightroom\Mimic.lrplugin"
$required = @("Info.lua","Init.lua","Shutdown.lua","Bridge.lua","Capabilities.lua","Catalog.lua","Develop.lua","Snapshots.lua","Metadata.lua","Commands.lua","Logger.lua","Json.lua","Version.lua","PluginInfoProvider.lua","MenuStatus.lua","MenuReconnect.lua")
$missing = $required | Where-Object { -not (Test-Path (Join-Path $src $_)) }
if ($missing) { throw "Plugin is missing: $($missing -join ', ')" }
$info = Get-Content (Join-Path $src "Info.lua") -Raw
$ver = Get-Content (Join-Path $src "Version.lua") -Raw
$pkg = (Get-Content (Join-Path $repo "package.json") -Raw | ConvertFrom-Json).version
if ($info -notmatch [regex]::Escape("display = `"$pkg`"")) { throw "Info.lua VERSION.display != $pkg (run pnpm sync-version)" }
if ($ver -notmatch [regex]::Escape("version = `"$pkg`"")) { throw "Version.lua != $pkg (run pnpm sync-version)" }
Write-Host "plugin package ok ($($required.Count) files, version $pkg)"
if ($Verify) { return }
if (-not $Destination) { $Destination = Join-Path $env:LOCALAPPDATA "Formicaria\Mimic\plugin\Mimic.lrplugin" }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
Copy-Item (Join-Path $src "*") $Destination -Recurse -Force
Write-Host "Copied to $Destination"
Write-Host "In Lightroom Classic: File > Plug-in Manager > Add > choose that folder."
