# Runs everything CI runs, in the same order. Stops at the first red step.
# Works in Windows PowerShell 5.1 and PowerShell 7.
param([switch]$Quick)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Run($name, $exe, $arguments, $cwd) {
  Write-Host "`n== $name" -ForegroundColor Cyan
  if ($cwd) { Push-Location $cwd }
  try {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "FAILED: $name (exit $LASTEXITCODE)" }
  } finally {
    if ($cwd) { Pop-Location }
  }
}

Run "version consistency" node @("scripts/sync-version.mjs", "--check")
Run "prettier" pnpm @("format:check")
Run "typecheck" pnpm @("typecheck")
Run "eslint" pnpm @("lint")
Run "vitest" pnpm @("test")
Run "frontend build" pnpm @("--filter", "@mimic/desktop", "build")
Run "cargo fmt" cargo @("fmt", "--all", "--", "--check")
Run "cargo clippy" cargo @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
Run "cargo test" cargo @("test", "--workspace")
Run "ruff check" uv @("run", "ruff", "check", ".") "engine"
Run "ruff format" uv @("run", "ruff", "format", "--check", ".") "engine"
Run "pytest" uv @("run", "pytest") "engine"
Write-Host "`nAll green." -ForegroundColor Green
