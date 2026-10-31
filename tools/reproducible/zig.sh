#!/bin/sh
# Stands in for zig in the fixed build environment (tools/reproducible/Dockerfile), where it is installed as
# /usr/local/bin/zig: it runs the real zig (/opt/ziglang/zig) with the same arguments, except one. cargo-zigbuild asks
# zig for the Mac targets "x86_64-macos-none" and "aarch64-macos-none", which zig takes as macOS 13 and newer: so the
# program would no longer start on older Macs. Here they name the oldest macOS versions that Rust builds for, and that
# SeedLock's Mac program has always started on: 10.12 (Sierra) on Intel chips, 11 (Big Sur) on Apple chips.
for arg do
    shift
    case $arg in
        x86_64-macos-none) arg=x86_64-macos.10.12-none ;;
        aarch64-macos-none) arg=aarch64-macos.11.0-none ;;
    esac
    set -- "$@" "$arg"
done
exec /opt/ziglang/zig "$@"
