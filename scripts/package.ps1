#!/usr/bin/env pwsh
# Builds a release and packs it for Windows in dist/: a zip to unpack anywhere,
# and an installer made with Inno Setup 6.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
. (Join-Path $PSScriptRoot 'env.ps1')

if (Get-ChildItem assets/icons -Filter *.svg -ErrorAction SilentlyContinue) {
    Write-Warning 'The program icons in assets/icons go into this build: read assets/icons/NOTICE.md before passing it on.'
}
$iscc = @(
    $env:ISCC
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Write-Error 'Inno Setup 6 was not found: set ISCC, or put ISCC.exe on PATH.'
}

if (Test-Path assets/windows/rex.png) {
    Write-Warning "Rex's own icon in assets/windows goes into this build: read assets/windows/NOTICE.md before passing it on."
}
# The program's icon, from Rex's or else Rexy's drawing. The build and the installer take it.
Write-Host 'cargo run --release --locked --example icon' -ForegroundColor Cyan
cargo run --release --locked --example icon
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host 'cargo build --release --locked' -ForegroundColor Cyan
cargo build --release --locked
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$version = ($metadata.packages | Where-Object name -EQ 'rexy').version

# What both hold: the program, and what is said of it and of what it bundles.
$dist = (New-Item -ItemType Directory -Force dist).FullName
$name = "rexy-$version-windows-x64"
$stage = Join-Path $dist $name
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage 'licenses') | Out-Null
Copy-Item (Join-Path $env:CARGO_TARGET_DIR 'release\rexy.exe') $stage
Copy-Item README.md, THIRD_PARTY.md $stage
Copy-Item assets/fonts/*-LICENSE.txt (Join-Path $stage 'licenses')

$zip = Join-Path $dist "$name.zip"
Compress-Archive -Path $stage -DestinationPath $zip -Force
Write-Host $zip -ForegroundColor Green

Write-Host 'ISCC installer/rexy.iss' -ForegroundColor Cyan
& $iscc /Q "/DAppVersion=$version" "/DStage=$stage" installer/rexy.iss
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
Remove-Item -Recurse -Force $stage
Write-Host (Join-Path $dist "rexy-$version-setup.exe") -ForegroundColor Green
