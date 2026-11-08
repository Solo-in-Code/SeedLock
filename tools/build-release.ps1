# Builds what a release offers for download, in SeedLock's fixed build environment (tools/reproducible/Dockerfile),
# on Windows with Docker Desktop (or Podman): the ready-made programs for Linux, Windows and the Mac, and the phone
# app before it is signed. Into the folder "dist":
#
#   dist\seedlock-VERSION-linux-x86_64          Linux (64-bit Intel or AMD): a static program, for any such Linux
#   dist\seedlock-VERSION-windows-x86_64.exe    Windows (64-bit Intel or AMD)
#   dist\seedlock-VERSION-macos                 macOS: one file for Macs with Apple chips and with Intel chips
#   dist\seedlock-VERSION-android-unsigned.apk  the phone app, which tools\sign-apk.ps1 then signs for a release
#
# Wherever and whenever they are built, they come out the same, byte for byte (docs/DEVELOPMENT.md, "Reproducible
# builds"). So anyone can check a release: in its source code (its "Source code (zip)", unpacked), with the
# release's SHA256SUMS.txt put into that folder (and, to check the phone app too, the release's
# seedlock-VERSION-android.apk), in PowerShell or in Command Prompt:
#
#   powershell -ExecutionPolicy Bypass -File tools\build-release.ps1 SHA256SUMS.txt
#
# builds them and says, for each program, whether it is the same file as the release's, and whether the release's
# app is this build with a signature added and nothing else. ("-ExecutionPolicy Bypass" lets Windows run this one
# script; it changes no setting.) With -NoCache, everything is built again, without what Docker kept from earlier
# builds (to see the same files come out again). Docker Desktop must be running, with about 15 GB of free disk space.
# The first time it downloads about 3 GB (a Debian system, Rust, zig, Java, Gradle and Android's tools) and takes a
# while; after that, a build takes some minutes.
#
# With -GradleChecksums instead (after a version in the app's build files changed), it writes Gradle's list of the
# files it downloads for the app, each with its checksum, anew in the same environment, into
# android\gradle\verification-metadata.xml: the app's build checks every file against it (docs/DEVELOPMENT.md,
# "Checksums of the app's libraries").
# Linux and macOS: tools/build-release.sh.

param([string]$Sums, [switch]$NoCache, [switch]$GradleChecksums)

$ErrorActionPreference = 'Stop'

function Fail([string]$message) {
    [Console]::Error.WriteLine($message)
    exit 1
}

# Whether a command runs and succeeds, its output hidden. (With 'Stop', Windows PowerShell 5 would stop the script
# at the first line such a command writes to its error output.)
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

function Get-Sha256([string]$path) {
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
}

# What helps when the environment's build stopped
function Stop-Build([string]$engine) {
    $running = 'Is Docker Desktop running?'
    if ($engine -eq 'podman') { $running = "Is Podman's machine running (podman machine start)?" }
    Fail ("`nThe build stopped (see the messages above). $running If it said ""no space left on device"": free " +
          "some disk space (about 15 GB are needed). If it could not download something: check the internet " +
          "connection and try again. If it said ""Dependency verification failed"": a file that Gradle downloaded " +
          "for the app is not the one in its list of checksums (android\gradle\verification-metadata.xml); do not " +
          "build the app with it, and see ""Checksums of the app's libraries"" in docs/DEVELOPMENT.md.")
}

$root = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $root
try {
    $version = $null
    foreach ($line in (Get-Content -LiteralPath 'Cargo.toml')) {
        if ($line -match '^version = "(.*)"$') { $version = $Matches[1]; break }
    }
    if (-not $version) { Fail 'The version was not found in Cargo.toml.' }
    if ($GradleChecksums -and $Sums) {
        Fail "With -GradleChecksums, this script writes Gradle's list and compares nothing: give it no other file."
    }
    if ($Sums -and -not (Test-Path -LiteralPath $Sums -PathType Leaf)) {
        Fail "There is no file ""$Sums"" here: put the release's SHA256SUMS.txt into SeedLock's folder first."
    }
    # Docker, when it can build here: it is running, with BuildKit (its part "buildx"). Otherwise Podman.
    $engine = $null
    if ((Test-Run 'docker' @('info')) -and (Test-Run 'docker' @('buildx', 'version'))) {
        $engine = 'docker'
    } elseif (Get-Command 'podman' -ErrorAction SilentlyContinue) {
        $engine = 'podman'
    } elseif (Get-Command 'docker' -ErrorAction SilentlyContinue) {
        $engine = 'docker'    # it says itself what is missing
    }
    if (-not $engine) {
        Fail ("This script needs Docker Desktop (docs.docker.com/desktop) or Podman, which run SeedLock's fixed " +
              "build environment. Install one of them, start it, then run this script again.")
    }
    $env:DOCKER_BUILDKIT = '1'
    $cache = @()
    if ($NoCache) { $cache = @('--no-cache') }
    $metadata = 'android\gradle\verification-metadata.xml'

    # With -GradleChecksums: Gradle's list of checksums for the app, written anew (Dockerfile, target gradle-checksums)
    if ($GradleChecksums) {
        $out = Join-Path 'dist' 'gradle-checksums'
        Remove-Item -LiteralPath $out -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host ("Writing Gradle's list of the files it downloads for SeedLock's app, each with its checksum, in " +
                    "the fixed build environment ($engine) ...")
        & $engine build @cache --platform linux/amd64 -f tools/reproducible/Dockerfile --target gradle-checksums `
            --output type=local,dest=dist/gradle-checksums .
        if ($LASTEXITCODE -ne 0) { Stop-Build $engine }
        $list = Join-Path $out 'verification-metadata.xml'
        $report = Join-Path $out 'report.txt'
        foreach ($file in @($list, $report)) {
            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { Fail "$file is missing: something went wrong." }
        }
        Copy-Item -LiteralPath $list -Destination $metadata -Force
        Write-Host ''
        foreach ($line in (Get-Content -LiteralPath $report)) { Write-Host $line }
        Write-Host ''
        Write-Host ("Written into ${metadata}: the app's build checks every file that Gradle downloads against it. " +
                    "Look through what changed before you build (docs/DEVELOPMENT.md, ""Checksums of the app's " +
                    "libraries"").")
        return
    }
    if (-not (Test-Path -LiteralPath $metadata -PathType Leaf)) {
        Fail ("$metadata is missing: without it, Gradle would not check the files it downloads for the app. Write " +
              "it first: powershell -ExecutionPolicy Bypass -File tools\build-release.ps1 -GradleChecksums")
    }

    $programs =@("seedlock-$version-linux-x86_64", "seedlock-$version-windows-x86_64.exe", "seedlock-$version-macos")
    $app = "seedlock-$version-android-unsigned.apk"
    $releaseApp = "seedlock-$version-android.apk"
    $releaseUnsigned = "seedlock-$version-android-release-without-signature.apk"
    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    foreach ($file in ($programs + @($app, $releaseUnsigned))) {
        # so that a failed build leaves no older file behind
        Remove-Item -LiteralPath (Join-Path 'dist' $file) -Force -ErrorAction SilentlyContinue
    }

    Write-Host ("Building SeedLock $version for Linux, Windows, the Mac and Android, in the fixed build environment " +
                "($engine) ...")
    $appCompared = Test-Path -LiteralPath $releaseApp -PathType Leaf
    if ($appCompared) { Write-Host "($releaseApp is here: the release's app is compared with the one built.)" }
    # --platform linux/amd64: the same environment on every computer. --output: what is built comes out into dist.
    & $engine build @cache --platform linux/amd64 -f tools/reproducible/Dockerfile --output type=local,dest=dist .
    if ($LASTEXITCODE -ne 0) { Stop-Build $engine }

    $expected = @{}
    if ($Sums) {
        foreach ($line in (Get-Content -LiteralPath $Sums)) {
            # the checksum, two spaces (or a space and *), the file's name
            if ($line -match '^([0-9a-fA-F]{64}) [ *](.+)$') {
                $expected[$Matches[2].Trim()] = $Matches[1].ToLowerInvariant()
            }
        }
    }
    Write-Host ''
    $same = 0
    $different = 0
    foreach ($program in $programs) {
        $path = Join-Path 'dist' $program
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            Fail "dist\$program is missing: something went wrong."
        }
        $sum = Get-Sha256 $path
        Write-Host "dist\$program"
        Write-Host "  SHA-256: $sum"
        if ($Sums) {
            if (-not $expected.ContainsKey($program)) {
                Write-Host "  NOT in $Sums (is the list of another version?)"
                $different++
            } elseif ($expected[$program] -eq $sum) {
                Write-Host "  the same as in $Sums"
                $same++
            } else {
                Write-Host "  DIFFERENT from $Sums, which says $($expected[$program])"
                $different++
            }
        }
    }

    $appPath = Join-Path 'dist' $app
    if (-not (Test-Path -LiteralPath $appPath -PathType Leaf)) { Fail "dist\$app is missing: something went wrong." }
    $appSum = Get-Sha256 $appPath
    Write-Host "dist\$app"
    Write-Host "  SHA-256: $appSum"
    if ($appCompared) {
        $releaseUnsignedPath = Join-Path 'dist' $releaseUnsigned
        if (-not (Test-Path -LiteralPath $releaseUnsignedPath -PathType Leaf)) {
            Fail "dist\$releaseUnsigned is missing: something went wrong."
        }
        if ((Get-Sha256 $releaseUnsignedPath) -eq $appSum) {
            Write-Host "  the same as $releaseApp (the release's app) without its signature"
        } else {
            Write-Host "  DIFFERENT from $releaseApp (the release's app) without its signature"
            $different++
        }
        if ($Sums) {
            if (-not $expected.ContainsKey($releaseApp)) {
                Write-Host "  $releaseApp is NOT in $Sums"
                $different++
            } elseif ($expected[$releaseApp] -eq (Get-Sha256 $releaseApp)) {
                Write-Host "  and $releaseApp is the file that $Sums names"
            } else {
                Write-Host "  but $releaseApp is NOT the file that $Sums names (it says $($expected[$releaseApp]))"
                $different++
            }
        }
    } elseif ($Sums) {
        Write-Host "  (to compare the release's app too, put its $releaseApp into this folder)"
    }

    Write-Host ''
    if (-not $Sums -and -not $appCompared) {
        Write-Host ('Ready. For a release, sign the app next: powershell -ExecutionPolicy Bypass -File ' +
                    'tools\sign-apk.ps1 PATH\seedlock-android.jks (see "Making a release" in docs/DEVELOPMENT.md).')
    } elseif ($different -eq 0) {
        if ($Sums -and $appCompared) {
            Write-Host ("All $same programs are the same, byte for byte, as the ones in ${Sums}, and the release's app " +
                        'is the app built here with its signature added and nothing else: they were built from this ' +
                        'source code.')
        } elseif ($Sums) {
            Write-Host ("All $same programs are the same, byte for byte, as the ones in ${Sums}: they were built from " +
                        'this source code.')
        } else {
            Write-Host ("The release's app is the app built here, with its signature added and nothing else: it was " +
                        'built from this source code.')
        }
    } else {
        $which = "$different files are"
        if ($different -eq 1) { $which = '1 file is' }
        Fail ("$which NOT the same as the release's. Check that this is the source code of the same release (Source " +
              'code (zip) of its page), unchanged; see "Reproducible builds" in docs/DEVELOPMENT.md.')
    }
}
finally {
    Pop-Location
}
