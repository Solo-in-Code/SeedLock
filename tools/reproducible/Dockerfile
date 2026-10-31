# The fixed environment in which SeedLock's ready-made programs are built: for Linux, Windows and the Mac. Built in
# it, the programs come out the same, byte for byte, wherever and whenever they are built. So anyone can check that a
# release's programs come from its source code: docs/DEVELOPMENT.md, "Reproducible builds". tools/build-release.sh
# (and .ps1) runs it; by hand, in SeedLock's folder:
#
#   docker build --platform linux/amd64 -f tools/reproducible/Dockerfile --output type=local,dest=dist .
#
# (or podman in place of docker). Everything that goes into the programs is fixed below, each one by its checksum:
# the Debian system, Rust (rustup checks the compiler and its libraries against the checksums Rust publishes), zig
# (the C compiler and linker for the three systems) and cargo-zigbuild (which lets Cargo build with zig). SeedLock's
# own libraries are fixed by Cargo.lock. The source code is copied in and built in the same folder, /seedlock, with
# the same settings, every time: tools/reproducible/build-programs.sh.
#
# When a pinned version changes here, the programs change too: see "Reproducible builds" for how to update them.

FROM docker.io/library/debian:trixie-20261005-slim@sha256:918311b7b6c4c6f68b232ba516584925f6c78ad82b6fd534b98979df6438e483 AS build

# curl and unzip download and unpack the tools below. gcc and libc6-dev build Cargo's build scripts, which run here
# while building and are not part of the programs.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev unzip \
 && rm -rf /var/lib/apt/lists/*

# Rust 1.97.0, with its libraries for the three systems. rustup-init 1.29.1 is checked by its SHA-256, the same one
# as in Rust's own Docker images (github.com/rust-lang/docker-rust).
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN curl --proto '=https' --tlsv1.2 -fsSLo /tmp/rustup-init \
        https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-unknown-linux-gnu/rustup-init \
 && echo 'dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71  /tmp/rustup-init' | sha256sum -c - \
 && chmod 755 /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain 1.97.0 \
 && rm /tmp/rustup-init \
 && rustup target add x86_64-unknown-linux-musl x86_64-pc-windows-gnu aarch64-apple-darwin x86_64-apple-darwin \
 && rustc --version

# zig 0.15.2, the official build that the Zig Software Foundation also publishes on PyPI (pypi.org/project/ziglang),
# and cargo-zigbuild 0.23.4 (pypi.org/project/cargo-zigbuild), each checked by its SHA-256. A wheel is a zip file.
RUN curl --proto '=https' --tlsv1.2 -fsSLo /tmp/zig.whl \
        https://files.pythonhosted.org/packages/db/81/f9f5c7fd68d80b8edff2a641a1c95ac4125356387d298374227040b8e1e8/ziglang-0.15.2-py3-none-manylinux_2_12_x86_64.manylinux2010_x86_64.musllinux_1_1_x86_64.whl \
 && echo '806316453d1ede7174ed3fefb8e5348154629089c3bb5fe812dfd0e172fac130  /tmp/zig.whl' | sha256sum -c - \
 && unzip -q /tmp/zig.whl 'ziglang/*' -d /opt \
 && chmod 755 /opt/ziglang/zig \
 && rm /tmp/zig.whl \
 && curl --proto '=https' --tlsv1.2 -fsSLo /tmp/cargo-zigbuild.whl \
        https://files.pythonhosted.org/packages/95/2b/3fbef7958e7902b1c0119e1a028528df6a5d28bae11282ee550689cb8b59/cargo_zigbuild-0.23.4-py3-none-manylinux_2_5_x86_64.manylinux1_x86_64.whl \
 && echo '0a149a2a07b9513d422efa9cc23e1add9741f08abe86845fa418eaeb4cca7931  /tmp/cargo-zigbuild.whl' | sha256sum -c - \
 && unzip -q -j /tmp/cargo-zigbuild.whl 'cargo_zigbuild-0.23.4.data/scripts/cargo-zigbuild' -d /usr/local/bin \
 && chmod 755 /usr/local/bin/cargo-zigbuild \
 && rm /tmp/cargo-zigbuild.whl \
 && /opt/ziglang/zig version \
 && cargo-zigbuild --version

# SeedLock's source code (.dockerignore leaves out what is not source code, and your backups, should they be there),
# always in the same folder. tools/reproducible/zig.sh stands in for zig (it sets the oldest macOS versions).
COPY . /seedlock
WORKDIR /seedlock
ENV CARGO_ZIGBUILD_ZIG_PATH=/usr/local/bin/zig
RUN install -m 755 tools/reproducible/zig.sh /usr/local/bin/zig \
 && sh tools/reproducible/build-programs.sh

# What comes out: the three programs, into the folder given with --output (dist).
FROM scratch AS programs
COPY --from=build /seedlock/dist/ /
