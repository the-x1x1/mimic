# Builds the Python engine into a single-folder PyInstaller bundle that the
# Tauri bundle picks up from apps/desktop/src-tauri/resources/engine/.
$ErrorActionPreference = "Stop"
$repo = Join-Path $PSScriptRoot ".."
Set-Location (Join-Path $repo "engine")
uv sync --all-extras --dev
$out = Join-Path $repo "apps\desktop\src-tauri\resources\engine"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Force -Path $out | Out-Null
uv run pyinstaller --noconfirm --clean --name mimic-engine --onedir --console `
  --collect-all mimic_engine --collect-submodules sklearn --collect-submodules scipy `
  --collect-binaries onnxruntime `
  --distpath (Join-Path $repo "engine\dist") --workpath (Join-Path $repo "engine\build") `
  --specpath (Join-Path $repo "engine\build") `
  (Join-Path $repo "engine\src\mimic_engine\__main__.py")
Copy-Item (Join-Path $repo "engine\dist\mimic-engine\*") $out -Recurse -Force
# The directory is emptied above; its one tracked file is written back so a
# developer who packages locally does not show up with a deleted README.
Set-Content -Path (Join-Path $out "README.txt") -Value "Packaged engine lands here (scripts/package-engine.ps1). Not committed."
$exe = Join-Path $out "mimic-engine.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $out "mimic-engine" }
& $exe --version
if ($LASTEXITCODE -ne 0) { throw "packaged engine failed to run" }
# Protocol smoke: hello over stdio must answer with the right protocol version.
$hello = '{"protocolVersion":1,"requestId":"smoke","method":"engine.hello","params":{}}' + "`n" + '{"protocolVersion":1,"requestId":"bye","method":"engine.shutdown","params":{}}' + "`n"
# `& $exe serve` yields one string per output line; join before matching, because
# `-notmatch` on an array returns the non-matching lines (truthy) instead of a boolean.
$resp = (($hello | & $exe serve) -join "`n")
if ($LASTEXITCODE -ne 0) { throw "packaged engine exited with $LASTEXITCODE" }
if ($resp -notmatch '"engineVersion"' -or $resp -notmatch '"protocolVersion":1') { throw "packaged engine did not answer engine.hello: $resp" }
Write-Host "engine packaged at $out"
