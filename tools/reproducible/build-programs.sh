#!/bin/sh
# Runs inside the fixed build environment (tools/reproducible/Dockerfile), never on its own: it builds SeedLock's three
# ready-made programs into dist/ and checks them. Everything here is the same for everyone who builds them (the
# folder, the tools, the settings), so the programs come out the same, byte for byte.
set -eu

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "The version was not found in Cargo.toml." >&2
    exit 1
fi

# Rust writes the full path of each source file into the program, for its error messages. The libraries' paths read
# ~/.cargo/registry/..., as in the releases before; SeedLock's own are relative to its folder.
sep=$(printf '\037')    # separates the flags in CARGO_ENCODED_RUSTFLAGS
paths="--remap-path-prefix=$CARGO_HOME=~/.cargo$sep--remap-path-prefix=$PWD=."
unset RUSTFLAGS CARGO_BUILD_RUSTFLAGS
export CARGO_TARGET_DIR="$PWD/target"
# zig and cargo-zigbuild keep their files in target too (not in the home folder), so that they go with it at the end
export ZIG_GLOBAL_CACHE_DIR="$PWD/target/zig" ZIG_LOCAL_CACHE_DIR="$PWD/target/zig" \
    CARGO_ZIGBUILD_CACHE_DIR="$PWD/target"

cargo fetch --locked

# build TARGET [FLAG ...]: the program for one system, with these compiler flags besides the paths
build() {
    target=$1
    shift
    flags=$paths
    for flag do
        flags="$flags$sep$flag"
    done
    echo "Building SeedLock $version for $target ..."
    CARGO_ENCODED_RUSTFLAGS="$flags" cargo zigbuild --release --locked --offline -p seedlock-cli --target "$target"
}

# Linux: a static program (musl inside), which runs on any x86-64 Linux, old or new. -pie: position-independent, so
# that it is loaded at a random address each time (Rust asks for that with -static-pie, which zig does not take).
build x86_64-unknown-linux-musl -Clink-arg=-pie

# Windows: built with the free MinGW tools (zig brings them), so it needs no Visual C++ runtime on the computer.
# Windows's protections, a random address and data never run as code, zig turns on by itself; --nxcompat asks for
# the second one anyway (check-programs checks both). -Brepro: the linker writes no date into the program.
build x86_64-pc-windows-gnu -Clink-arg=-Wl,--nxcompat -Clink-arg=-Wl,-Brepro

# The Mac: one file for both kinds of processor (Apple chips and Intel). zig removes the symbols itself (-x), because
# it computes the file's identifier (its UUID) from the finished file: when Rust removed them after zig, the
# identifier came from a file with temporary file names in it, and differed every time.
export CARGO_PROFILE_RELEASE_STRIP=false
build universal2-apple-darwin -Clink-arg=-Wl,-x -Clink-arg=-s
unset CARGO_PROFILE_RELEASE_STRIP

mkdir -p dist
cp target/x86_64-unknown-linux-musl/release/seedlock "dist/seedlock-$version-linux-x86_64"
cp target/x86_64-pc-windows-gnu/release/seedlock.exe "dist/seedlock-$version-windows-x86_64.exe"
cp target/universal2-apple-darwin/release/seedlock "dist/seedlock-$version-macos"
chmod 755 dist/*

# What each program must be (the kind of file, the system libraries it needs, the protections, the Mac program's
# oldest macOS and its signature, nothing of the test builds, no folder of this environment): seedlock-tools.
cargo build --release --locked --offline -p seedlock-tools
target/release/seedlock-tools check-programs dist "$version"
(cd dist && sha256sum "seedlock-$version-linux-x86_64" "seedlock-$version-windows-x86_64.exe" "seedlock-$version-macos")

# What the builds left behind (about a GB) need not be kept with the programs.
rm -rf target
