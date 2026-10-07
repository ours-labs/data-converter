$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$oldFlags = $env:CARGO_ENCODED_RUSTFLAGS
$oldTarget = $env:CARGO_TARGET_DIR
try {
    $paths = @($env:USERPROFILE, $projectRoot) | Where-Object { $_ } | Select-Object -Unique
    $flags = @()
    foreach ($path in $paths) {
        $flags += "--remap-path-prefix=$path=/_build"
        $flags += "--remap-path-prefix=$($path.Replace('\', '/'))=/_build"
    }
    $flags += @('-C', 'strip=debuginfo')
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'local/public-build'
    Push-Location $projectRoot
    try {
        cargo build --locked --release
        if ($LASTEXITCODE -ne 0) { throw 'CLI build failed' }
        cargo build --manifest-path src-tauri/Cargo.toml --locked --release --features custom-protocol
        if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed' }
    } finally { Pop-Location }
    Write-Output (Join-Path $env:CARGO_TARGET_DIR 'release')
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $oldFlags
    $env:CARGO_TARGET_DIR = $oldTarget
}
