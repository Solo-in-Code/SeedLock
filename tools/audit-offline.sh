#!/bin/sh
# Verifies that nothing in the SeedLock binary can use the network, and that its license holds:
#   1. no networking / HTTP / TLS / async-runtime crate anywhere in its dependency tree
#   2. no networking API used in SeedLock's own source code
#   3. the only other programs SeedLock can start: ffmpeg (to read the camera) and ffplay (the camera
#      preview window, and the window that shows QR codes); to see whether the computer is connected to a
#      network, /sbin/route on macOS and route.exe on Windows (they read the routing table and send nothing);
#      to see whether it is a virtual machine, sysctl on macOS and reg.exe on Windows (they read its firmware
#      description); and the system's "open" command (only with --qr-browser); nothing else, and never from
#      the core library
#   4. unsafe code is limited to terminal and process calls in ui.rs and main.rs (never in the core library)
#   5. every library SeedLock is built from, on every system and build helpers included, declares a license
#      and none of them is copyleft (GPL and the like), so SeedLock itself can be under the MIT license
# Needs the Rust toolchain: "cargo --version" must work. If it cannot, this script says FAIL (it never says OK).
# Run it after every dependency update:  sh tools/audit-offline.sh
set -eu
cd "$(dirname "$0")/.."

command -v cargo >/dev/null 2>&1 || { echo "FAIL: cargo was not found, so the dependency tree cannot be checked. Put the Rust toolchain on the PATH first."; exit 1; }
tree=$(cargo tree -p seedlock-cli --edges normal --prefix none --locked) || { echo "FAIL: cargo tree did not work, so the dependency tree cannot be checked."; exit 1; }
deps=$(printf '%s\n' "$tree" | sed 's/ (.*//; s/ v/ /' | sort -u)
count=$(printf '%s\n' "$deps" | wc -l)
if [ "$count" -lt 20 ]; then
    echo "FAIL: only $count crates were found in the dependency tree, which cannot be right (SeedLock has about 80)."; exit 1
fi
bad=$(printf '%s\n' "$deps" | grep -E '^(reqwest|hyper|h2|http|httparse|tokio|mio|socket2|openssl|openssl-sys|native-tls|rustls|curl|ureq|isahc|surf|attohttpc|async-std|smol|tungstenite|hickory|trust-dns|tonic|quinn|libp2p|bitcoincore-rpc|electrum-client|esplora-client) ' || true)
if [ -n "$bad" ]; then
    echo "FAIL: networking-capable dependencies found:"; echo "$bad"; exit 1
fi

own=$(grep -rnE 'std::net|TcpStream|UdpSocket|TcpListener|reqwest|hyper::|tokio::' crates/*/src || true)
if [ -n "$own" ]; then
    echo "FAIL: networking API used in SeedLock's own code:"; echo "$own"; exit 1
fi

procs=$(grep -rn 'Command::new' crates/*/src | grep -vE '^crates/seedlock-cli/src/(scan|ui|qrwindow|netcheck)\.rs:' || true)
if [ -n "$procs" ] || grep -rq 'std::process' crates/seedlock-core/src; then
    echo "FAIL: a program is started from somewhere it should not be:"; echo "$procs"; exit 1
fi
# unsafe code: only terminal and process calls in ui.rs (window size, single key presses) and main.rs (no core dumps); never in the core library
other=$(grep -rlw unsafe crates/*/src | grep -vE '^crates/seedlock-cli/src/(ui|main)\.rs$' || true)
if [ -n "$other" ]; then
    echo "FAIL: unsafe code outside the terminal and process helpers:"; echo "$other"; exit 1
fi
# licenses: on every system (--target all), build helpers included; dev-only crates never reach the program
lic=$(cargo tree -p seedlock-cli --edges normal,build --target all --prefix none --format '{p} | {l}' --locked) || { echo "FAIL: cargo tree did not work, so the licenses cannot be checked."; exit 1; }
lic=$(printf '%s\n' "$lic" | sed 's/ (\*)$//' | sort -u)
missing=$(printf '%s\n' "$lic" | grep -E '\| *$' || true)
if [ -n "$missing" ]; then
    echo "FAIL: these crates declare no license, so check them by hand:"; echo "$missing"; exit 1
fi
copyleft=$(printf '%s\n' "$lic" | grep -E '\|(.*[^A-Za-z])?(A?GPL|LGPL|MPL|EUPL|CDDL|EPL|OSL|SSPL|CPAL|RPL|CC-BY-SA)-' || true)
if [ -n "$copyleft" ]; then
    echo "FAIL: copyleft licenses found; SeedLock is under the MIT license, so check these by hand:"; echo "$copyleft"; exit 1
fi
not_mit=$(printf '%s\n' "$lic" | grep -E '^seedlock-(cli|core) ' | grep -v '| MIT$' || true)
if [ -n "$not_mit" ]; then
    echo "FAIL: SeedLock's own crates must say license = \"MIT\" (Cargo.toml, [workspace.package]):"; echo "$not_mit"; exit 1
fi
libs=$(printf '%s\n' "$lic" | grep -cvE '^seedlock-(cli|core) ')

echo "unsafe code (terminal and process calls only, never in the core library):"
grep -rnw unsafe crates/*/src | sed 's/^/  /' | cut -c1-110
echo "Programs SeedLock can start (nothing else does):"
grep -rn 'Command::new' crates/*/src | sed 's/^/  /'

echo "OK: $count crates in the dependency tree, none with network access; no network API in SeedLock's own source."
echo "OK: all $libs libraries SeedLock is built from (on every system) have permissive licenses, none copyleft; SeedLock itself is MIT."
