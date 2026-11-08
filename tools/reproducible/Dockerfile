# The fixed environment in which SeedLock's ready-made programs (for Linux, Windows and the Mac) and its phone app
# are built. Built in it, they come out the same, byte for byte, wherever and whenever they are built (the phone
# app before it is signed). So anyone can check that a release's programs and app come from its source code:
# docs/DEVELOPMENT.md, "Reproducible builds". tools/build-release.sh (and .ps1) runs it; by hand, in SeedLock's folder:
#
#   docker build --platform linux/amd64 -f tools/reproducible/Dockerfile --output type=local,dest=dist .
#
# (or podman in place of docker). Everything that goes into them is fixed below, each one by its checksum: the
# Debian system, Rust (rustup checks the compiler and its libraries against the checksums Rust publishes), zig (the
# C compiler and linker for the three systems), cargo-zigbuild (which lets Cargo build with zig), Java, Gradle and
# Android's command-line tools (whose sdkmanager installs the Android SDK's parts by their exact versions, checking
# each against Google's checksums). SeedLock's own libraries are fixed by Cargo.lock, the app's by their versions
# in android/ and by Gradle's list of their checksums (android/gradle/verification-metadata.xml). The source code is
# copied in and built in the same folder, /seedlock, with the same settings, every time:
# tools/reproducible/build-programs.sh and build-app.sh.
#
# When a pinned version changes here, the programs and the app change too: see "Reproducible builds" for how to
# update them.

FROM docker.io/library/debian:trixie-20261005-slim@sha256:918311b7b6c4c6f68b232ba516584925f6c78ad82b6fd534b98979df6438e483 AS base

# curl and unzip download and unpack the tools below. gcc and libc6-dev build Cargo's build scripts, which run here
# while building and are not part of the programs.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev unzip \
 && rm -rf /var/lib/apt/lists/*

# Rust 1.97.0, with its libraries for the three systems and the phone's three kinds of processor. rustup-init 1.29.1
# is checked by its SHA-256, the same one as in Rust's own Docker images (github.com/rust-lang/docker-rust).
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
        aarch64-linux-android armv7-linux-androideabi x86_64-linux-android \
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

# ---- The three programs ----------------------------------------------------------------------------------------
# SeedLock's source code (.dockerignore leaves out what is not source code, and your backups, should they be there),
# always in the same folder. tools/reproducible/zig.sh stands in for zig (it sets the oldest macOS versions).
FROM base AS programs-build
COPY . /seedlock
WORKDIR /seedlock
ENV CARGO_ZIGBUILD_ZIG_PATH=/usr/local/bin/zig
RUN install -m 755 tools/reproducible/zig.sh /usr/local/bin/zig \
 && sh tools/reproducible/build-programs.sh

# ---- The tools that build the phone app ------------------------------------------------------------------------
# Java 17 (Eclipse Temurin 17.0.20+8, github.com/adoptium/temurin17-binaries), Gradle 8.11.1 (gradle.org), and
# Android's command-line tools 15859902 (developer.android.com/studio), each checked by its SHA-256. Their sdkmanager
# then installs the Android SDK's parts by their exact versions, checking each against Google's checksums: Android
# 15's platform (35), the build tools 34.0.0 (which also hold apksigner, for tools/sign-apk.sh), and the NDK r28c
# (28.2.13676358), whose compilers build the Rust library for phones. Running it accepts the Android SDK's license
# (as Android Studio asks you to). The same versions are written in android/app/build.gradle.kts.
FROM base AS android-env
ENV JAVA_HOME=/opt/jdk \
    ANDROID_HOME=/opt/android-sdk \
    ANDROID_NDK_VERSION=28.2.13676358 \
    PATH=/opt/jdk/bin:/opt/gradle/bin:/opt/android-sdk/cmdline-tools/latest/bin:/opt/android-sdk/build-tools/34.0.0:/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN curl --proto '=https' --tlsv1.2 -fsSLo /tmp/jdk.tar.gz \
        https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.20%2B8/OpenJDK17U-jdk_x64_linux_hotspot_17.0.20_8.tar.gz \
 && echo 'be7668bc030d578b83d6d5ef9221d6d6729bbbca8cf94a7d52e16ac68b5a5a35  /tmp/jdk.tar.gz' | sha256sum -c - \
 && tar -xzf /tmp/jdk.tar.gz -C /opt \
 && mv /opt/jdk-17.0.20+8 /opt/jdk \
 && rm /tmp/jdk.tar.gz \
 && java -version \
 && curl --proto '=https' --tlsv1.2 -fsSLo /tmp/gradle.zip \
        https://services.gradle.org/distributions/gradle-8.11.1-bin.zip \
 && echo 'f397b287023acdba1e9f6fc5ea72d22dd63669d59ed4a289a29b1a76eee151c6  /tmp/gradle.zip' | sha256sum -c - \
 && unzip -q /tmp/gradle.zip -d /opt \
 && mv /opt/gradle-8.11.1 /opt/gradle \
 && rm /tmp/gradle.zip \
 && gradle --version --no-daemon
RUN curl --proto '=https' --tlsv1.2 -fsSLo /tmp/cmdline-tools.zip \
        https://dl.google.com/android/repository/commandlinetools-linux-15859902_latest.zip \
 && echo '4e4c464f145a7512b57d088ac6c278c03c9eea610886b35a5e0804e74eedf583  /tmp/cmdline-tools.zip' | sha256sum -c - \
 && mkdir -p /opt/android-sdk/cmdline-tools \
 && unzip -q /tmp/cmdline-tools.zip -d /opt/android-sdk/cmdline-tools \
 && mv /opt/android-sdk/cmdline-tools/cmdline-tools /opt/android-sdk/cmdline-tools/latest \
 && rm /tmp/cmdline-tools.zip \
 && yes | sdkmanager --licenses > /dev/null \
 && sdkmanager --install "platforms;android-35" "build-tools;34.0.0" "ndk;$ANDROID_NDK_VERSION" \
 && sdkmanager --list_installed \
 && apksigner --version

# ---- The phone app ---------------------------------------------------------------------------------------------
# The same source code in the same folder; the app comes out before it is signed (tools/sign-apk.sh signs it).
FROM android-env AS app-build
COPY . /seedlock
WORKDIR /seedlock
RUN sh tools/reproducible/build-app.sh

# ---- Not part of a release: Gradle's list of checksums for the app ---------------------------------------------
# Only with --target gradle-checksums (tools/build-release.sh --gradle-checksums), after a version in the app's build
# files changed: the list of every file that Gradle downloads for the app, each with its SHA-256, written anew by the
# same build as the release's and compared with the list before (android/gradle/verification-metadata.xml, which
# build-app.sh above makes Gradle check every file against). It comes out alone, with a report.
FROM android-env AS gradle-checksums-build
COPY . /seedlock
WORKDIR /seedlock
RUN sh tools/reproducible/build-app.sh --gradle-checksums

FROM scratch AS gradle-checksums
COPY --from=gradle-checksums-build /seedlock/dist/ /

# ---- What comes out: the three programs and the app, into the folder given with --output (dist) ----------------
FROM scratch AS release
COPY --from=programs-build /seedlock/dist/ /
COPY --from=app-build /seedlock/dist/ /
