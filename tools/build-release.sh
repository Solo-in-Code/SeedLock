#!/bin/sh
# Builds the ready-made programs that a release offers for download, for Linux, Windows and the Mac, in SeedLock's
# fixed build environment (tools/reproducible/Dockerfile), and puts them into the folder "dist":
#
#   dist/seedlock-VERSION-linux-x86_64         Linux (64-bit Intel or AMD): a static program, for any such Linux
#   dist/seedlock-VERSION-windows-x86_64.exe   Windows (64-bit Intel or AMD)
#   dist/seedlock-VERSION-macos                macOS: one file for Macs with Apple chips and with Intel chips
#
# Wherever and whenever they are built, they come out the same, byte for byte (docs/DEVELOPMENT.md, "Reproducible
# builds"). So anyone can check a release's programs: in the release's source code (its "Source code (zip)",
# unpacked), with the release's SHA256SUMS.txt put into that folder,
#
#   sh tools/build-release.sh SHA256SUMS.txt
#
# builds them and says, for each one, whether it is the same file as the release's. With --no-cache first, Docker
# builds everything again, without what it kept from earlier builds (to see the same programs come out again).
#
# Needs Docker or Podman (on Linux, or on a Mac with Docker Desktop; on Windows: tools/build-release.ps1), about 4 GB
# of free disk space, and an internet connection. The first time it downloads about 1 GB (a Debian system, Rust and
# zig) and takes a while; after that, Docker keeps those, and a build takes some minutes.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "The version was not found in Cargo.toml." >&2
    exit 1
fi
cache=""
if [ "${1:-}" = "--no-cache" ]; then
    cache=--no-cache
    shift
fi
sums=${1:-}
if [ -n "$sums" ] && [ ! -f "$sums" ]; then
    echo "There is no file \"$sums\" here: put the release's SHA256SUMS.txt into SeedLock's folder first." >&2
    exit 1
fi
# Docker, when it can build here: its service answers, and it has BuildKit (the part "buildx", which some Linux
# packages of Docker leave out). Otherwise Podman, which needs neither.
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1 && docker buildx version >/dev/null 2>&1; then
    engine=docker
elif command -v podman >/dev/null 2>&1; then
    engine=podman
elif command -v docker >/dev/null 2>&1; then
    engine=docker    # it says itself what is missing, and the hints below what to do
else
    cat >&2 <<'MSG'
This script needs Docker or Podman, which run SeedLock's fixed build environment. Install one of them:
  Debian, Ubuntu : sudo apt install podman            (or Docker: docs.docker.com/engine/install)
  Arch           : sudo pacman -S podman
  Fedora         : sudo dnf install podman
  Mac            : Docker Desktop (docs.docker.com/desktop), then start it
then run this script again.
MSG
    exit 1
fi

programs="seedlock-$version-linux-x86_64 seedlock-$version-windows-x86_64.exe seedlock-$version-macos"
mkdir -p dist
for program in $programs; do
    rm -f "dist/$program"    # so that a failed build leaves no older file behind
done

echo "Building SeedLock $version for Linux, Windows and the Mac, in the fixed build environment ($engine) ..."
# --platform linux/amd64: the same environment on every computer, also on a Mac with an Apple chip (which runs it
# more slowly). --output: the programs come out into dist; nothing else of the environment does.
# shellcheck disable=SC2086 # $cache is empty or one word
if ! DOCKER_BUILDKIT=1 "$engine" build $cache --platform linux/amd64 -f tools/reproducible/Dockerfile \
        --output type=local,dest=dist .; then
    if [ "$engine" = docker ]; then
        cat >&2 <<'MSG'

The build stopped (see the messages above). What Docker said, and what helps:
  "permission denied": add yourself to the group "docker" (sudo usermod -aG docker $USER, then log out and in
      again), or install Podman, which needs no group; this script uses it when Docker cannot build.
  "Cannot connect to the Docker daemon": start Docker (on a Mac: Docker Desktop; on Linux: sudo systemctl start
      docker).
  "the buildx component is missing": install it (Debian, Ubuntu: sudo apt install docker-buildx; Arch: sudo pacman
      -S docker-buildx), or install Podman.
  It could not download something: check the internet connection and try again.
MSG
    else
        cat >&2 <<'MSG'

The build stopped (see the messages above). What Podman said, and what helps:
  "insufficient UIDs or GIDs": give your user its own range of them (sudo usermod --add-subuids 100000-165535
      --add-subgids 100000-165535 $USER, then podman system migrate).
  It could not download something: check the internet connection and try again.
MSG
    fi
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    hash() { sha256sum "$1" | cut -d' ' -f1; }
else
    hash() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi
echo
same=0
different=0
for program in $programs; do
    if [ ! -f "dist/$program" ]; then
        echo "dist/$program is missing: something went wrong." >&2
        exit 1
    fi
    sum=$(hash "dist/$program")
    echo "dist/$program"
    echo "  SHA-256: $sum"
    if [ -n "$sums" ]; then
        # a line of the list: the checksum, two spaces (or a space and *), the file's name
        expected=$(awk -v name="$program" '$2 == name || $2 == "*" name { print $1; exit }' "$sums")
        if [ -z "$expected" ]; then
            echo "  NOT in $sums (is the list of another version?)"
            different=$((different + 1))
        elif [ "$expected" = "$sum" ]; then
            echo "  the same as in $sums"
            same=$((same + 1))
        else
            echo "  DIFFERENT from $sums, which says $expected"
            different=$((different + 1))
        fi
    fi
done
echo
if [ -z "$sums" ]; then
    echo "Ready. For a release, bring these programs and the phone app together to make SHA256SUMS.txt: see \"Making a"
    echo "release\" in docs/DEVELOPMENT.md."
elif [ "$different" -eq 0 ]; then
    echo "All $same programs are the same, byte for byte, as the ones in $sums: they were built from this source code."
else
    if [ "$different" -eq 1 ]; then
        echo "1 of the 3 programs is NOT the same as in $sums."
    else
        echo "$different of the 3 programs are NOT the same as in $sums."
    fi
    echo "Check that this is the source code of the same release (Source code (zip) of its page), unchanged; see"
    echo "\"Reproducible builds\" in docs/DEVELOPMENT.md."
    exit 1
fi
