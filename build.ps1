param(
    [string]$CargoExecutable = 'cargo',
    [switch]$Release,
    [switch]$Test
)
$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath $PSScriptRoot
try {
    if ($Test) {
        & $CargoExecutable fmt --all -- --check
        if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
        & $CargoExecutable clippy --locked --workspace --all-features --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
        & $CargoExecutable test --locked --workspace --all-features --all-targets
        if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
        & $CargoExecutable test --locked --workspace --all-features --doc
        if ($LASTEXITCODE -ne 0) { throw 'Documentation tests failed' }
    }
    $cargoArguments = @('build', '--locked', '-p', 'openscan-egui', '--features', 'standalone', '--example', 'standalone')
    if ($Release) { $cargoArguments += '--release' }
    & $CargoExecutable @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
}
finally {
    Pop-Location
}
