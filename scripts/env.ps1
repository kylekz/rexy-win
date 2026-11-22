# What building on Windows needs, set for the scripts that dot-source this.

# libghostty-vt builds Ghostty with Zig.
if (-not $env:ZIG) {
    $zig = Get-Command zig.exe -ErrorAction SilentlyContinue
    if ($zig) {
        $env:ZIG = $zig.Source
    } else {
        $packages = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages'
        $found = Get-ChildItem $packages -Directory -Filter 'zig.zig_*' -ErrorAction SilentlyContinue |
            ForEach-Object { Join-Path $_.FullName 'zig-x86_64-windows-0.17.1\zig.exe' } |
            Where-Object { Test-Path $_ } |
            Select-Object -First 1
        if (-not $found) {
            Write-Error 'Zig 0.17.1 was not found: set ZIG, or put zig.exe on PATH.'
        }
        $env:ZIG = $found
    }
}
# A short path: Ghostty's build goes deep enough to pass Windows' limit otherwise.
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = 'C:\t\rexy'
}
