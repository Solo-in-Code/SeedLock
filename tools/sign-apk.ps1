# Signs the phone app that tools\build-release.ps1 built (dist\seedlock-VERSION-android-unsigned.apk) with
# SeedLock's Android key, for a release: with the apksigner of the Android SDK in the fixed build environment
# (tools/reproducible/Dockerfile), run without any network, on Windows with Docker Desktop (or Podman). The key store
# is only read, and its passwords are typed when apksigner asks for them (they are never stored). It writes
# dist\seedlock-VERSION-android.apk, checks its signature, and, where Rust is installed, checks it with seedlock-tools
# check-apk: the release's app must be the app built in the fixed environment with a signature added, and nothing
# else (docs/DEVELOPMENT.md, Making a release). In PowerShell or in Command Prompt:
#
#   powershell -ExecutionPolicy Bypass -File tools\sign-apk.ps1 E:\seedlock-android.jks
#
# Linux and macOS: tools/sign-apk.sh.

param([string]$Keystore)

$ErrorActionPreference = 'Stop'

function Fail([string]$message) {
    [Console]::Error.WriteLine($message)
    exit 1
}

# Whether a command runs and succeeds, its output hidden (see tools\build-release.ps1).
function Test-Run([string]$name, [string[]]$arguments) {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) { return $false }
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $name @arguments *> $null
        return ($LASTEXITCODE -eq 0)
    } catch {
        return $false
    } finally {
        $ErrorActionPreference = $previous
    }
}

$root = Split-Path -Parent $PSScriptRoot
if (-not $Keystore -or -not (Test-Path -LiteralPath $Keystore -PathType Leaf)) {
    Fail ("Give SeedLock's key store (DEVELOPMENT.md, The Android signing key), for example:`n" +
          '  powershell -ExecutionPolicy Bypass -File tools\sign-apk.ps1 E:\seedlock-android.jks')
}
$keyPath = (Resolve-Path -LiteralPath $Keystore).Path
$keyFolder = Split-Path -Parent $keyPath
$keyFile = Split-Path -Leaf $keyPath
Push-Location -LiteralPath $root
try {
    $version = $null
    foreach ($line in (Get-Content -LiteralPath 'Cargo.toml')) {
        if ($line -match '^version = "(.*)"$') { $version = $Matches[1]; break }
    }
    if (-not $version) { Fail 'The version was not found in Cargo.toml.' }
    $unsigned = "seedlock-$version-android-unsigned.apk"
    $signed = "seedlock-$version-android.apk"
    if (-not (Test-Path -LiteralPath (Join-Path 'dist' $unsigned) -PathType Leaf)) {
        Fail "dist\$unsigned is not there: build it first, with tools\build-release.ps1"
    }
    if (Test-Path -LiteralPath (Join-Path 'dist' $signed)) {
        Fail ("dist\$signed is there already: nothing is replaced. Move it away first if a new one should take " +
              "its place.")
    }
    # the same choice as tools\build-release.ps1's
    $engine = $null
    if ((Test-Run 'docker' @('info')) -and (Test-Run 'docker' @('buildx', 'version'))) {
        $engine = 'docker'
    } elseif (Get-Command 'podman' -ErrorAction SilentlyContinue) {
        $engine = 'podman'
    } elseif (Get-Command 'docker' -ErrorAction SilentlyContinue) {
        $engine = 'docker'
    }
    if (-not $engine) { Fail 'This script needs Docker Desktop or Podman, as tools\build-release.ps1 does.' }

    # The environment's Android tools, as tools\build-release.ps1 built them (Docker kept them: this takes moments).
    Write-Host "Preparing the fixed build environment's Android tools ($engine) ..."
    $env:DOCKER_BUILDKIT = '1'
    & $engine build --platform linux/amd64 --target android-env -t seedlock-android-env `
        -f tools/reproducible/Dockerfile .
    if ($LASTEXITCODE -ne 0) {
        Fail "`nThe environment could not be prepared (see the messages above). Is $engine running?"
    }

    Write-Host ''
    Write-Host ("Signing dist\$unsigned with the key ""seedlock"" in $keyFile. apksigner asks for the key store's " +
                "password (and for the key's, if it is another one):")
    # --network none: nothing in the container can reach any network while the key is there. The key store's folder
    # is there only to be read. The signature schemes: v2, as SeedLock's app has always been signed (Android 7.0+).
    $dist = Join-Path $root 'dist'
    & $engine run --rm -it --network none --platform linux/amd64 -e HOME=/tmp `
        -v "${dist}:/dist" -v "${keyFolder}:/key:ro" `
        seedlock-android-env apksigner sign --ks "/key/$keyFile" --ks-key-alias seedlock `
        --v1-signing-enabled false --v2-signing-enabled true --v3-signing-enabled false --v4-signing-enabled false `
        --out "/dist/$signed" "/dist/$unsigned"
    if ($LASTEXITCODE -ne 0) { Fail "`nThe app was not signed (see the messages above)." }

    Write-Host ''
    Write-Host 'Its signature, as Android checks it:'
    & $engine run --rm --network none --platform linux/amd64 -e HOME=/tmp -v "${dist}:/dist:ro" `
        seedlock-android-env apksigner verify --verbose --print-certs "/dist/$signed"
    if ($LASTEXITCODE -ne 0) { Fail "`nIts signature does not check out (see the messages above)." }

    Write-Host ''
    if (Get-Command 'cargo' -ErrorAction SilentlyContinue) {
        & cargo run -q --locked -p seedlock-tools -- check-apk "dist/$signed"
        if ($LASTEXITCODE -ne 0) { exit 1 }
    } else {
        Write-Host 'Now check it, on a computer with Rust, in SeedLock''s folder (with dist copied there):'
        Write-Host "  cargo run --locked -p seedlock-tools -- check-apk dist/$signed"
    }
}
finally {
    Pop-Location
}
