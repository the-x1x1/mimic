# Verifies prerequisites and installs every dependency. Idempotent.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Need($name, $cmd, $hint) {
  if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
    Write-Host "MISSING: $name ($cmd). $hint" -ForegroundColor Red
    $script:missing = $true
  } else {
    Write-Host "ok: $name -> $((Get-Command $cmd).Source)"
  }
}
$missing = $false
Need "Node.js 22+" "node" "https://nodejs.org"
Need "pnpm 10+" "pnpm" "corepack enable; corepack prepare pnpm@10.28.0 --activate"
Need "Rust (cargo)" "cargo" "https://rustup.rs"
Need "uv" "uv" "https://docs.astral.sh/uv/getting-started/installation/"
if ($missing) { throw "Install the missing prerequisites, then re-run .\scripts\bootstrap.ps1" }

$node = (node -p "process.versions.node.split('.')[0]")
if ([int]$node -lt 22) { throw "Node 22+ required, found $(node --version)" }

Write-Host "`n== JavaScript" -ForegroundColor Cyan
pnpm install --frozen-lockfile
Write-Host "`n== Python engine" -ForegroundColor Cyan
Push-Location engine
uv python install 3.12
uv sync --all-extras --dev
Pop-Location
Write-Host "`n== Rust" -ForegroundColor Cyan
cargo fetch
Write-Host "`nBootstrap complete. Next: .\scripts\dev.ps1" -ForegroundColor Green
