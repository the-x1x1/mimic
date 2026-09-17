# Starts the full development app: Vite + Tauri shell; the shell spawns the
# engine via `uv run` from ./engine automatically. One command, no manual
# orchestration of three processes.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
if (-not (Test-Path "node_modules")) { throw "Run .\scripts\bootstrap.ps1 first" }
if (-not (Test-Path "engine\.venv")) { throw "Engine environment missing. Run .\scripts\bootstrap.ps1 first" }
pnpm --filter @mimic/desktop tauri dev
