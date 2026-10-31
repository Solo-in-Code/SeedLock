# Builds the Rust part of SeedLock's Android app on Windows, before Android Studio builds the app itself:
#   - libseedlock_ffi.so for the three kinds of processor phones have, into app\src\main\jniLibs\
#   - the Kotlin code that calls it (package org.seedlock.ffi), into app\src\main\java\
#   - the license notices of the libraries the app is built from, which the app shows (About SeedLock and its
#     licenses), into app\src\main\assets\notices.txt
#
# In SeedLock's folder, in PowerShell or in Command Prompt:
#
#   powershell -ExecutionPolicy Bypass -File android\build-rust.ps1
#
# Needs Rust installed with rustup (README, Part 1, Way B), cargo-ndk (cargo install cargo-ndk), the
# Android NDK (Android Studio: Settings > Languages & Frameworks > Android SDK > SDK Tools > NDK (Side by
# side)) and an internet connection. Run it again whenever the Rust code changed. android\README.md says more.

$ErrorActionPreference = 'Stop'

function Fail([string]$message) {
    [Console]::Error.WriteLine($message)
    exit 1
}

$root = Split-Path -Parent $PSScriptRoot

# A path inside SeedLock's folder, one folder name at a time.
function Under([string[]]$names) {
    $path = $root
    foreach ($name in $names) { $path = Join-Path $path $name }
    return $path
}

Push-Location -LiteralPath $root
try {
    if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
        Fail 'This script needs Rust installed with rustup (README, Part 1, Way B).'
    }
    if (-not (Get-Command cargo-ndk -ErrorAction SilentlyContinue)) {
        Fail 'This script needs cargo-ndk. Install it once with:  cargo install cargo-ndk'
    }
    if (-not $env:USERPROFILE) { Fail 'The variable USERPROFILE (your user folder) is not set.' }
    & rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
    if ($LASTEXITCODE -ne 0) { Fail 'rustup could not add the Android targets (is this computer online?).' }

    # Rust writes the full path of each library's source files into the program, and that path holds this
    # computer's user name: --remap-path-prefix writes ~ in its place.
    foreach ($variable in 'RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS') {
        [Environment]::SetEnvironmentVariable($variable, $null)
    }
    $folders = @($env:USERPROFILE)
    $flags = "--remap-path-prefix=$($env:USERPROFILE)=~"
    if ($env:CARGO_HOME) {
        $folders += $env:CARGO_HOME
        $flags += [string][char]0x1f + "--remap-path-prefix=$($env:CARGO_HOME)=~\.cargo"
    }
    $jnilibs = Under 'android', 'app', 'src', 'main', 'jniLibs'
    if (Test-Path -LiteralPath $jnilibs) { Remove-Item -LiteralPath $jnilibs -Recurse -Force }
    Write-Host 'Building libseedlock_ffi.so for 64-bit phones (arm64-v8a), 32-bit phones (armeabi-v7a) and the emulator (x86_64) ...'
    $env:CARGO_ENCODED_RUSTFLAGS = $flags
    & cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -P 24 -o $jnilibs build --profile phone --locked -p seedlock-ffi
    $built = $LASTEXITCODE
    [Environment]::SetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', $null)
    if ($built -ne 0) { Fail 'The build stopped with an error (see the messages above).' }

    # The Kotlin code is written from a build for this computer: a debug build keeps the description that
    # uniffi-bindgen reads, and the code is the same for every kind of processor.
    Write-Host 'Writing the Kotlin code that calls it, and the license notices ...'
    & cargo build --locked -p seedlock-ffi -p uniffi-bindgen -p seedlock-tools
    if ($LASTEXITCODE -ne 0) { Fail 'The build stopped with an error (see the messages above).' }
    $generated = Under 'android', 'app', 'src', 'main', 'java', 'org', 'seedlock', 'ffi'
    if (Test-Path -LiteralPath $generated) { Remove-Item -LiteralPath $generated -Recurse -Force }
    $bindgen = Under 'target', 'debug', 'uniffi-bindgen.exe'
    $hostLib = Under 'target', 'debug', 'seedlock_ffi.dll'
    & $bindgen generate --no-format --language kotlin --out-dir (Under 'android', 'app', 'src', 'main', 'java') $hostLib
    if ($LASTEXITCODE -ne 0) { Fail 'uniffi-bindgen stopped with an error (see the messages above).' }
    # the notices of exactly the libraries just built in (crates\seedlock-tools)
    $notices = Under 'android', 'app', 'src', 'main', 'assets', 'notices.txt'
    & (Under 'target', 'debug', 'seedlock-tools.exe') notices --android --out $notices
    if ($LASTEXITCODE -ne 0) { Fail 'seedlock-tools could not write the license notices (see the messages above).' }

    foreach ($lib in (Get-ChildItem -LiteralPath $jnilibs -Recurse -Filter 'libseedlock_ffi.so')) {
        $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($lib.FullName))
        foreach ($folder in $folders) {
            $path = $folder.TrimEnd([char[]]('\', '/'))
            foreach ($form in ($path + '\'), ($path.Replace('\', '/') + '/')) {
                if ($text.IndexOf($form, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                    Fail "$($lib.FullName) still contains the path $path (and with it your user name): something went wrong."
                }
            }
        }
    }
    Write-Host ''
    Write-Host 'Ready:'
    Get-ChildItem -LiteralPath $jnilibs -Recurse -Filter 'libseedlock_ffi.so' | ForEach-Object { Write-Host "  $($_.FullName)  $($_.Length) bytes" }
    Write-Host "  $notices  $((Get-Item -LiteralPath $notices).Length) bytes"
    Write-Host 'Now open the folder "android" in Android Studio and build the app (android\README.md).'
}
finally {
    Pop-Location
}
