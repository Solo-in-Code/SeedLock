#!/bin/sh
# Builds what a release offers for download, in SeedLock's fixed build environment (tools/reproducible/Dockerfile):
# the ready-made programs for Linux, Windows and the Mac, and the phone app before it is signed. Into "dist":
#
#   dist/seedlock-VERSION-linux-x86_64          Linux (64-bit Intel or AMD): a static program, for any such Linux
#   dist/seedlock-VERSION-windows-x86_64.exe    Windows (64-bit Intel or AMD)
#   dist/seedlock-VERSION-macos                 macOS: one file for Macs with Apple chips and with Intel chips
#   dist/seedlock-VERSION-android-unsigned.apk  the phone app, which tools/sign-apk.sh then signs for a release
#
# Wherever and whenever they are built, they come out the same, byte for byte (docs/DEVELOPMENT.md, "Reproducible
# builds"). So anyone can check a release: in its source code (its "Source code (zip)", unpacked), with the
# release's SHA256SUMS.txt put into that folder (and, to check the phone app too, the release's
# seedlock-VERSION-android.apk),
#
#   sh tools/build-release.sh SHA256SUMS.txt
#
# builds them and says, for each program, whether it is the same file as the release's, and whether the release's
# app is this build with a signature added and nothing else. With --no-cache first, everything is built again,
# without what Docker kept from earlier builds (to see the same files come out again).
#
# Needs Docker or Podman (on Linux, or on a Mac with Docker Desktop; on Windows: tools/build-release.ps1), about
# 15 GB of free disk space, and an internet connection. The first time it downloads about 3 GB (a Debian system,
# Rust, zig, Java, Gradle and Android's tools) and takes a while; after that, Docker keeps those, and a build takes
# some minutes.
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
app="seedlock-$version-android-unsigned.apk"
release_app="seedlock-$version-android.apk"
release_unsigned="seedlock-$version-android-release-without-signature.apk"
mkdir -p dist
for file in $programs "$app" "$release_unsigned"; do
    rm -f "dist/$file"    # so that a failed build leaves no older file behind
done

echo "Building SeedLock $version for Linux, Windows, the Mac and Android, in the fixed build environment ($engine) ..."
if [ -f "$release_app" ]; then
    echo "($release_app is here: the release's app is compared with the one built.)"
fi
# --platform linux/amd64: the same environment on every computer, also on a Mac with an Apple chip (which runs it
# more slowly). --output: what is built comes out into dist; nothing else of the environment does.
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
  "no space left on device": free some disk space (about 15 GB are needed), and try again.
  It could not download something: check the internet connection and try again.
MSG
    else
        cat >&2 <<'MSG'

The build stopped (see the messages above). What Podman said, and what helps:
  "insufficient UIDs or GIDs": give your user its own range of them (sudo usermod --add-subuids 100000-165535
      --add-subgids 100000-165535 $USER, then podman system migrate).
  "no space left on device": free some disk space (about 15 GB are needed), and try again.
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
# the checksum that a list gives a file: the checksum, two spaces (or a space and *), the file's name
listed() { awk -v name="$1" '$2 == name || $2 == "*" name { print $1; exit }' "$sums"; }
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
        expected=$(listed "$program")
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

if [ ! -f "dist/$app" ]; then
    echo "dist/$app is missing: something went wrong." >&2
    exit 1
fi
echo "dist/$app"
echo "  SHA-256: $(hash "dist/$app")"
app_compared=no
if [ -f "$release_app" ]; then
    app_compared=yes
    if [ ! -f "dist/$release_unsigned" ]; then
        echo "dist/$release_unsigned is missing: something went wrong." >&2
        exit 1
    fi
    if [ "$(hash "dist/$release_unsigned")" = "$(hash "dist/$app")" ]; then
        echo "  the same as $release_app (the release's app) without its signature"
    else
        echo "  DIFFERENT from $release_app (the release's app) without its signature"
        different=$((different + 1))
    fi
    if [ -n "$sums" ]; then
        expected=$(listed "$release_app")
        if [ -z "$expected" ]; then
            echo "  $release_app is NOT in $sums"
            different=$((different + 1))
        elif [ "$expected" = "$(hash "$release_app")" ]; then
            echo "  and $release_app is the file that $sums names"
        else
            echo "  but $release_app is NOT the file that $sums names (it says $expected)"
            different=$((different + 1))
        fi
    fi
elif [ -n "$sums" ]; then
    echo "  (to compare the release's app too, put its $release_app into this folder)"
fi

echo
if [ -z "$sums" ] && [ "$app_compared" = no ]; then
    echo "Ready. For a release, sign the app next: sh tools/sign-apk.sh /path/to/seedlock-android.jks (see \"Making a"
    echo "release\" in docs/DEVELOPMENT.md)."
elif [ "$different" -eq 0 ]; then
    if [ -n "$sums" ] && [ "$app_compared" = yes ]; then
        echo "All $same programs are the same, byte for byte, as the ones in $sums, and the release's app is the app"
        echo "built here with its signature added and nothing else: they were built from this source code."
    elif [ -n "$sums" ]; then
        echo "All $same programs are the same, byte for byte, as the ones in $sums: they were built from this source"
        echo "code."
    else
        echo "The release's app is the app built here, with its signature added and nothing else: it was built from"
        echo "this source code."
    fi
else
    if [ "$different" -eq 1 ]; then
        echo "1 file is NOT the same as the release's."
    else
        echo "$different files are NOT the same as the release's."
    fi
    echo "Check that this is the source code of the same release (Source code (zip) of its page), unchanged; see"
    echo "\"Reproducible builds\" in docs/DEVELOPMENT.md."
    exit 1
fi
