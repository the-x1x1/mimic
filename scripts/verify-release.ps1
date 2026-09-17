# Release verification.
#   -PreTag : checks that the repository is releasable (version consistency,
#             changelog, updater pubkey is not the development key).
#   -Tag vX.Y.Z : downloads the published GitHub Release assets and validates
#             latest.json (platform entry, signature present, URL resolvable).
param([switch]$PreTag, [string]$Tag)
$ErrorActionPreference = "Stop"
$repo = Join-Path $PSScriptRoot ".."
Set-Location $repo
$pkg = (Get-Content package.json -Raw | ConvertFrom-Json).version

if ($PreTag) {
  node scripts/sync-version.mjs --check
  if ($LASTEXITCODE -ne 0) { throw "version check failed" }
  $conf = Get-Content apps/desktop/src-tauri/tauri.conf.json -Raw | ConvertFrom-Json
  $devKey = Get-Content docs/DEV_UPDATER_PUBKEY.txt -Raw
  if ($conf.plugins.updater.pubkey.Trim() -eq $devKey.Trim() -and -not $pkg.Contains("alpha")) {
    throw "REFUSING: tauri.conf.json still uses the development updater public key. Generate a real keypair (pnpm tauri signer generate) before a non-alpha release."
  }
  if ($conf.plugins.updater.pubkey.Trim() -eq $devKey.Trim()) {
    Write-Host "WARNING: development updater key in use (allowed for alpha builds only)." -ForegroundColor Yellow
  }
  if (-not (Select-String -Path CHANGELOG.md -Pattern "## \[$([regex]::Escape($pkg))\]" -Quiet)) { throw "CHANGELOG has no section for $pkg" }
  if (-not (Select-String -Path docs/PROJECT_STATUS.md -Pattern ([regex]::Escape($pkg)) -Quiet)) { throw "docs/PROJECT_STATUS.md does not mention $pkg" }
  Write-Host "pre-tag verification ok for v$pkg"
  return
}

if (-not $Tag) { throw "Use -PreTag or -Tag vX.Y.Z" }
$base = "https://github.com/the-x1x1/mimic/releases/download/$Tag"
$tmp = Join-Path ([IO.Path]::GetTempPath()) "mimic-verify-$Tag"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
Invoke-WebRequest "$base/latest.json" -OutFile (Join-Path $tmp "latest.json")
$latest = Get-Content (Join-Path $tmp "latest.json") -Raw | ConvertFrom-Json
if ("v$($latest.version)" -ne $Tag -and $latest.version -ne $Tag.TrimStart("v")) { throw "latest.json version $($latest.version) != $Tag" }
$win = $latest.platforms.'windows-x86_64'
if (-not $win) { throw "latest.json has no windows-x86_64 platform" }
if (-not $win.signature -or $win.signature.Length -lt 64) { throw "windows-x86_64 signature missing" }
$head = Invoke-WebRequest $win.url -Method Head -MaximumRedirection 5
if ($head.StatusCode -ne 200) { throw "updater asset not downloadable: $($win.url)" }
Invoke-WebRequest "$base/SHA256SUMS.txt" -OutFile (Join-Path $tmp "SHA256SUMS.txt")
Write-Host "release $Tag verified: latest.json ok, signature present, asset reachable, checksums published"
