# Builds the ready-made SeedLock program for Windows that a release offers for download, and names it after the
# version (from Cargo.toml). The file goes into the folder "dist":
#
#   dist\seedlock-VERSION-windows-x86_64.exe   for 64-bit Windows on Intel and AMD processors
#
# It has the C runtime inside (the setting in .cargo\config.toml), so it also starts on a computer without
# Microsoft's "Visual C++ Redistributable", and it does not contain the user name of the computer that built it
# (see below). Start it in SeedLock's folder, in PowerShell or in Command Prompt:
#
#   powershell -ExecutionPolicy Bypass -File tools\build-release.ps1
#
# ("-ExecutionPolicy Bypass" lets Windows run this one script; it changes no setting.) It needs Rust installed with
# rustup (README, Part 1, Way B) and an internet connection. Linux and macOS: tools/build-release.sh.

$ErrorActionPreference = 'Stop'
$target = 'x86_64-pc-windows-msvc'

function Fail([string]$message) {
    [Console]::Error.WriteLine($message)
    exit 1
}

$root = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $root
try {
    $version = $null
    foreach ($line in (Get-Content -LiteralPath 'Cargo.toml')) {
        if ($line -match '^version = "(.*)"$') { $version = $Matches[1]; break }
    }
    if (-not $version) { Fail 'The version was not found in Cargo.toml.' }
    if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
        Fail ("This script needs Rust installed with rustup, the official installer (README, Part 1, Way B):`n" +
              "  https://rustup.rs`nthen open a new window and run this script again.")
    }
    if (-not $env:USERPROFILE) { Fail 'The variable USERPROFILE (your user folder) is not set.' }

    # The release programs are built the same way everywhere: into this folder's "target", and with only the
    # compiler flags of .cargo\config.toml plus the ones below. A RUSTFLAGS variable would replace the setting in
    # .cargo\config.toml, so this script removes such variables (for this build only).
    $env:CARGO_TARGET_DIR = Join-Path $root 'target'
    foreach ($variable in 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS',
                          'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS') {
        [Environment]::SetEnvironmentVariable($variable, $null)
    }

    # Rust writes the full path of each library's source files into the program, for its error messages, and that
    # path holds this computer's user name: C:\Users\NAME\.cargo\... --remap-path-prefix makes it write ~ in place
    # of the user folder (and of Cargo's folder, if CARGO_HOME puts it elsewhere). Cargo adds these flags to the
    # ones in .cargo\config.toml. The end of this script checks that the user name is gone.
    $folders = @($env:USERPROFILE)
    $flags = @("'--remap-path-prefix=$($env:USERPROFILE)=~'")
    if ($env:CARGO_HOME) {
        $folders += $env:CARGO_HOME
        $flags += "'--remap-path-prefix=$($env:CARGO_HOME)=~\.cargo'"
    }
    foreach ($folder in $folders) {
        if ($folder.Contains("'")) { Fail "The folder name $folder contains a ', which this script cannot pass on." }
    }
    $config = "target.$target.rustflags=[" + ($flags -join ', ') + ']'

    & rustup target add $target
    if ($LASTEXITCODE -ne 0) { Fail 'rustup could not add the Windows target (is this computer online?).' }
    Write-Host "Building SeedLock $version for Windows ($target) ..."
    & cargo build --release --locked --target $target --config $config
    if ($LASTEXITCODE -ne 0) { Fail 'The build stopped with an error (see the messages above).' }

    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $name = "seedlock-$version-windows-x86_64.exe"
    $out = Join-Path $dist $name
    Copy-Item -LiteralPath (Join-Path (Join-Path (Join-Path $env:CARGO_TARGET_DIR $target) 'release') 'seedlock.exe') `
              -Destination $out -Force

    $shown = (& $out --version | Out-String).Trim()
    if ($shown -ne "seedlock $version") {
        Fail "The new program says ""$shown"" instead of ""seedlock $version"": something went wrong."
    }
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($out))
    foreach ($folder in $folders) {
        $path = $folder.TrimEnd([char[]]('\', '/'))
        foreach ($form in ($path + '\'), ($path.Replace('\', '/') + '/')) {
            if ($text.IndexOf($form, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                Fail "The new program still contains the path $path (and with it your user name): something went wrong."
            }
        }
    }

    $sum = (Get-FileHash -Algorithm SHA256 -LiteralPath $out).Hash.ToLowerInvariant()
    Write-Host ''
    Write-Host "Ready: dist\$name"
    Write-Host "SHA-256: $sum"
    Write-Host ('Bring this file (and the other systems'' files) to one computer to make SHA256SUMS.txt: see ' +
                '"Making a release" in docs/DEVELOPMENT.md.')
}
finally {
    Pop-Location
}
