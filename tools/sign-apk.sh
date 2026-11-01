#!/bin/sh
# Signs the phone app that tools/build-release.sh built (dist/seedlock-VERSION-android-unsigned.apk) with SeedLock's
# Android key, for a release: with the apksigner of the Android SDK in the fixed build environment
# (tools/reproducible/Dockerfile), run without any network. The key store is only read, and its passwords are typed
# when apksigner asks for them (they are never stored). It writes dist/seedlock-VERSION-android.apk, checks its
# signature, and, where Rust is installed, checks it with seedlock-tools check-apk: the release's app must be the
# app built in the fixed environment with a signature added, and nothing else (docs/DEVELOPMENT.md, Making a
# release).
#
#   sh tools/sign-apk.sh /path/to/seedlock-android.jks
#
# Needs Docker or Podman, as tools/build-release.sh does. Windows: tools/sign-apk.ps1.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "The version was not found in Cargo.toml." >&2
    exit 1
fi
keystore=${1:-}
if [ -z "$keystore" ] || [ ! -f "$keystore" ]; then
    echo "Give SeedLock's key store (DEVELOPMENT.md, The Android signing key), for example:" >&2
    echo "  sh tools/sign-apk.sh /media/me/STICK/seedlock-android.jks" >&2
    exit 1
fi
unsigned="dist/seedlock-$version-android-unsigned.apk"
signed="dist/seedlock-$version-android.apk"
if [ ! -f "$unsigned" ]; then
    echo "$unsigned is not there: build it first, with sh tools/build-release.sh" >&2
    exit 1
fi
if [ -e "$signed" ]; then
    echo "$signed is there already: nothing is replaced. Move it away first if a new one should take its place." >&2
    exit 1
fi
# the same choice as tools/build-release.sh's
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1 && docker buildx version >/dev/null 2>&1; then
    engine=docker
elif command -v podman >/dev/null 2>&1; then
    engine=podman
elif command -v docker >/dev/null 2>&1; then
    engine=docker
else
    echo "This script needs Docker or Podman, as tools/build-release.sh does." >&2
    exit 1
fi

# The environment's Android tools, as tools/build-release.sh built them (Docker kept them: this takes moments).
echo "Preparing the fixed build environment's Android tools ($engine) ..."
DOCKER_BUILDKIT=1 "$engine" build --platform linux/amd64 --target android-env -t seedlock-android-env \
    -f tools/reproducible/Dockerfile .

key_folder=$(cd "$(dirname "$keystore")" && pwd)
key_file=$(basename "$keystore")
# Docker on Linux would write the signed app as root: as you, it stays yours (Podman does that by itself).
as_you=""
if [ "$engine" = docker ] && [ "$(uname -s)" = Linux ]; then
    as_you="--user $(id -u):$(id -g)"
fi
echo
echo "Signing $unsigned with the key \"seedlock\" in $key_file. apksigner asks for the key store's password (and"
echo "for the key's, if it is another one):"
# --network none: nothing in the container can reach any network while the key is there. The key store's folder
# is there only to be read. The signature schemes: v2, as SeedLock's app has always been signed (Android 7.0+).
# shellcheck disable=SC2086 # $as_you is empty or two words
"$engine" run --rm -it --network none --platform linux/amd64 $as_you -e HOME=/tmp \
    -v "$PWD/dist:/dist" -v "$key_folder:/key:ro" seedlock-android-env \
    apksigner sign --ks "/key/$key_file" --ks-key-alias seedlock \
        --v1-signing-enabled false --v2-signing-enabled true --v3-signing-enabled false --v4-signing-enabled false \
        --out "/$signed" "/$unsigned"

echo
echo "Its signature, as Android checks it:"
# shellcheck disable=SC2086
"$engine" run --rm --network none --platform linux/amd64 $as_you -e HOME=/tmp -v "$PWD/dist:/dist:ro" \
    seedlock-android-env apksigner verify --verbose --print-certs "/$signed"

echo
if command -v cargo >/dev/null 2>&1; then
    cargo run -q --locked -p seedlock-tools -- check-apk "$signed"
else
    echo "Now check it, on a computer with Rust, in SeedLock's folder (with dist/ copied there):"
    echo "  cargo run --locked -p seedlock-tools -- check-apk $signed"
fi
