# Full production build: engine bundle + frontend + Tauri installer (+ updater artifacts when signing env is set).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
node scripts/sync-version.mjs --check
pwsh -NoProfile -File scripts/package-engine.ps1
pnpm install --frozen-lockfile
pnpm --filter @mimic/desktop build
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and -not $env:TAURI_SIGNING_PRIVATE_KEY_PATH) {
  Write-Host "TAURI_SIGNING_PRIVATE_KEY not set: building installer WITHOUT updater artifacts (dev build)." -ForegroundColor Yellow
  pnpm --filter @mimic/desktop tauri build --no-bundle
  pnpm --filter @mimic/desktop tauri bundle
} else {
  pnpm --filter @mimic/desktop tauri build
}
Write-Host "Artifacts under target\release\bundle\" -ForegroundColor Green
