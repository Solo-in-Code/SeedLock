# Builds the ready-made programs that a release offers for download, for Linux, Windows and the Mac, in SeedLock's
# fixed build environment (tools/reproducible/Dockerfile), on Windows with Docker Desktop (or Podman), and puts them
# into the folder "dist":
#
#   dist\seedlock-VERSION-linux-x86_64         Linux (64-bit Intel or AMD): a static program, for any such Linux
#   dist\seedlock-VERSION-windows-x86_64.exe   Windows (64-bit Intel or AMD)
#   dist\seedlock-VERSION-macos                macOS: one file for Macs with Apple chips and with Intel chips
#
# Wherever and whenever they are built, they come out the same, byte for byte (docs/DEVELOPMENT.md, "Reproducible
# builds"). So anyone can check a release's programs: in the release's source code (its "Source code (zip)",
# unpacked), with the release's SHA256SUMS.txt put into that folder, in PowerShell or in Command Prompt:
#
#   powershell -ExecutionPolicy Bypass -File tools\build-release.ps1 SHA256SUMS.txt
#
# builds them and says, for each one, whether it is the same file as the release's. ("-ExecutionPolicy Bypass" lets
# Windows run this one script; it changes no setting.) With -NoCache, Docker builds everything again, without what it
# kept from earlier builds (to see the same programs come out again). Docker Desktop must be running, with about 4 GB
# of free disk space. The first time it downloads about 1 GB (a Debian system, Rust and zig) and takes a while; after
# that, a build takes some minutes.
# Linux and macOS: tools/build-release.sh.

param([string]$Sums, [switch]$NoCache)

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

$root = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $root
try {
    $version = $null
    foreach ($line in (Get-Content -LiteralPath 'Cargo.toml')) {
        if ($line -match '^version = "(.*)"$') { $version = $Matches[1]; break }
    }
    if (-not $version) { Fail 'The version was not found in Cargo.toml.' }
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

    $programs = @("seedlock-$version-linux-x86_64", "seedlock-$version-windows-x86_64.exe", "seedlock-$version-macos")
    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    foreach ($program in $programs) {
        # so that a failed build leaves no older file behind
        Remove-Item -LiteralPath (Join-Path 'dist' $program) -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Building SeedLock $version for Linux, Windows and the Mac, in the fixed build environment ($engine) ..."
    # --platform linux/amd64: the same environment on every computer. --output: the programs come out into dist.
    $env:DOCKER_BUILDKIT = '1'
    $cache = @()
    if ($NoCache) { $cache = @('--no-cache') }
    & $engine build @cache --platform linux/amd64 -f tools/reproducible/Dockerfile --output type=local,dest=dist .
    if ($LASTEXITCODE -ne 0) {
        $running = 'Is Docker Desktop running?'
        if ($engine -eq 'podman') { $running = "Is Podman's machine running (podman machine start)?" }
        Fail ("`nThe build stopped (see the messages above). $running If it could not download something: check " +
              "the internet connection and try again.")
    }

    $expected = @{}
    if ($Sums) {
        foreach ($line in (Get-Content -LiteralPath $Sums)) {
            # the checksum, two spaces (or a space and *), the file's name
            if ($line -match '^([0-9a-fA-F]{64}) [ *](.+)$') { $expected[$Matches[2].Trim()] = $Matches[1].ToLowerInvariant() }
        }
    }
    Write-Host ''
    $same = 0
    $different = 0
    foreach ($program in $programs) {
        $path = Join-Path 'dist' $program
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { Fail "dist\$program is missing: something went wrong." }
        $sum = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
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
    Write-Host ''
    if (-not $Sums) {
        Write-Host ('Ready. For a release, bring these programs and the phone app together to make SHA256SUMS.txt: ' +
                    'see "Making a release" in docs/DEVELOPMENT.md.')
    } elseif ($different -eq 0) {
        Write-Host "All $same programs are the same, byte for byte, as the ones in ${Sums}: they were built from this source code."
    } else {
        $which = "$different of the 3 programs are"
        if ($different -eq 1) { $which = '1 of the 3 programs is' }
        Fail ("$which NOT the same as in ${Sums}. Check that this is the source code of the same release (Source " +
              'code (zip) of its page), unchanged; see "Reproducible builds" in docs/DEVELOPMENT.md.')
    }
}
finally {
    Pop-Location
}
