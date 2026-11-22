#!/usr/bin/env pwsh
# Everything a change must pass on Windows: formatting, lints, tests and a
# release build. -Fast leaves out the release build.
param([switch]$Fast)

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
. (Join-Path $PSScriptRoot 'env.ps1')

$steps = @(
    @('fmt', '--check'),
    @('clippy', '--all-targets', '--', '-D', 'warnings'),
    @('test')
)
if (-not $Fast) {
    $steps += , @('build', '--release')
}
foreach ($step in $steps) {
    Write-Host "cargo $($step -join ' ')" -ForegroundColor Cyan
    & cargo @step
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
