$ErrorActionPreference = 'Stop'
& "$PSScriptRoot/build-public.ps1"
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
$projectRoot = Split-Path $PSScriptRoot -Parent
& "$projectRoot/.venv/Scripts/python.exe" "$PSScriptRoot/installer-notices.py"
if ($LASTEXITCODE -ne 0) { throw 'Notice generation failed' }
$previousTarget = $env:CARGO_TARGET_DIR
try {
  $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'local/public-build'
  Push-Location $projectRoot
  try {
    npx tauri bundle --config src-tauri/tauri.installer.conf.json --bundles nsis --features custom-protocol --ci
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
    $version = (Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
    $built = Get-ChildItem "$env:CARGO_TARGET_DIR/release/bundle/nsis/*_${version}_x64-setup.exe"
    if ($built.Count -ne 1) { throw 'Expected one x64 installer' }
    $target = Join-Path $projectRoot "dist/OURS-Data-Converter_${version}_x64-setup.exe"
    Copy-Item -LiteralPath $built.FullName -Destination $target
    Get-FileHash -Algorithm SHA256 -LiteralPath $target
  } finally { Pop-Location }
} finally { $env:CARGO_TARGET_DIR = $previousTarget }
