#!/bin/sh
# Verifies that nothing in the SeedLock binary can use the network, and that its license holds:
#   1. no networking / HTTP / TLS / async-runtime crate anywhere in its dependency tree
#   2. no networking API used in SeedLock's own source code
#   3. the only other programs SeedLock can start: ffmpeg (to read the camera) and ffplay (the camera
#      preview window, and the window that shows QR codes); to see whether the computer is connected to a
#      network, /sbin/route on macOS and route.exe on Windows (they read the routing table and send nothing);
#      to see whether it is a virtual machine, sysctl on macOS and reg.exe on Windows (they read its firmware
#      description); and the system's "open" command (only with --qr-browser); nothing else, and never from
#      the core library. (crates/seedlock-tools, the helper that builds the phone app's license notices, checks
#      its release APK on the computer that builds it, and checks the ready-made programs in the fixed build
#      environment, runs cargo, and the Linux program it checks, to see its version; it is never part of SeedLock.)
#   4. unsafe code is limited to terminal and process calls in ui.rs and main.rs (never in the core library)
#   5. every library SeedLock is built from, on every system and build helpers included, declares a license
#      and none of them is copyleft (GPL and the like), so SeedLock itself can be under the MIT license (a
#      library that lets you choose, such as "MIT OR LGPL-2.1-or-later", is taken under its permissive license)
#   6. the phone app (preview): checks 2 to 4 cover its Rust code too (crates/seedlock-ffi and seedlock-qr: no
#      network API, no program started, no unsafe code); its library has no networking crate either, on every
#      system (Android's included, whatever computer runs this), and its libraries' licenses are permissive
#      except UniFFI's own crates (MPL-2.0, see DEVELOPMENT.md); the app's manifest asks Android only for the
#      camera and the network state and takes INTERNET out (so Android refuses it any network), JNA is its
#      only other library, and its Kotlin code uses no network API
# Needs the Rust toolchain: "cargo --version" must work. If it cannot, this script says FAIL (it never says OK).
# Run it after every dependency update:  sh tools/audit-offline.sh
set -eu
cd "$(dirname "$0")/.."

net_crates='^(reqwest|hyper|h2|http|httparse|tokio|mio|socket2|openssl|openssl-sys|native-tls|rustls|curl|ureq|isahc|surf|attohttpc|async-std|smol|tungstenite|hickory|trust-dns|tonic|quinn|libp2p|bitcoincore-rpc|electrum-client|esplora-client) '

# Of the lines "crate vX.Y.Z | license", those with a copyleft license and no permissive one to choose instead:
# "MIT OR LGPL-2.1-or-later" passes (MIT is taken), "LGPL-2.1-or-later" and "MIT AND LGPL-2.1-or-later" do not.
copyleft_lines() {
    awk -v copyleft='[^A-Za-z](A?GPL|LGPL|MPL|EUPL|CDDL|EPL|OSL|SSPL|CPAL|RPL|CC-BY-SA)-' '{
        license = substr($0, index($0, " | ") + 3)
        if ((" " license) !~ copyleft) next
        if (license !~ / AND | WITH |[()]/) {
            n = split(license, choices, " OR ")
            for (i = 1; i <= n; i++) if ((" " choices[i]) !~ copyleft) next
        }
        print
    }'
}

command -v cargo >/dev/null 2>&1 || { echo "FAIL: cargo was not found, so the dependency tree cannot be checked. Put the Rust toolchain on the PATH first."; exit 1; }
tree=$(cargo tree -p seedlock-cli --edges normal --prefix none --locked) || { echo "FAIL: cargo tree did not work, so the dependency tree cannot be checked."; exit 1; }
deps=$(printf '%s\n' "$tree" | sed 's/ (.*//; s/ v/ /' | sort -u)
count=$(printf '%s\n' "$deps" | wc -l)
if [ "$count" -lt 20 ]; then
    echo "FAIL: only $count crates were found in the dependency tree, which cannot be right (SeedLock has about 80)."; exit 1
fi
bad=$(printf '%s\n' "$deps" | grep -E "$net_crates" || true)
if [ -n "$bad" ]; then
    echo "FAIL: networking-capable dependencies found:"; echo "$bad"; exit 1
fi

own=$(grep -rnE 'std::net|TcpStream|UdpSocket|TcpListener|reqwest|hyper::|tokio::' crates/*/src || true)
if [ -n "$own" ]; then
    echo "FAIL: networking API used in SeedLock's own code:"; echo "$own"; exit 1
fi

procs=$(grep -rn 'Command::new' crates/*/src | grep -vE '^crates/seedlock-cli/src/(scan|ui|qrwindow|netcheck)\.rs:|^crates/seedlock-tools/src/notices\.rs:.*Command::new\(cargo\(\)\)|^crates/seedlock-tools/src/programs\.rs:.*Command::new\(&path\)\.arg\("--version"\)' || true)
if [ -n "$procs" ] || grep -rq 'std::process' crates/seedlock-core/src crates/seedlock-ffi/src crates/seedlock-qr/src; then
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
copyleft=$(printf '%s\n' "$lic" | copyleft_lines)
if [ -n "$copyleft" ]; then
    echo "FAIL: copyleft licenses found; SeedLock is under the MIT license, so check these by hand:"; echo "$copyleft"; exit 1
fi
not_mit=$(printf '%s\n' "$lic" | grep -E '^seedlock-(cli|core) ' | grep -v '| MIT$' || true)
if [ -n "$not_mit" ]; then
    echo "FAIL: SeedLock's own crates must say license = \"MIT\" (Cargo.toml, [workspace.package]):"; echo "$not_mit"; exit 1
fi
libs=$(printf '%s\n' "$lic" | grep -cvE '^seedlock-(cli|core) ')

# 6. the phone app: its Rust library, on every system (--target all), so that Android's crates are checked
#    whatever computer runs this
ffi_tree=$(cargo tree -p seedlock-ffi --edges normal --target all --prefix none --locked) || { echo "FAIL: cargo tree did not work, so the phone app's dependency tree cannot be checked."; exit 1; }
ffi_deps=$(printf '%s\n' "$ffi_tree" | sed 's/ (.*//; s/ v/ /' | sort -u)
ffi_count=$(printf '%s\n' "$ffi_deps" | wc -l)
if [ "$ffi_count" -lt 20 ]; then
    echo "FAIL: only $ffi_count crates were found in the phone app's dependency tree, which cannot be right (it has about 115)."; exit 1
fi
bad=$(printf '%s\n' "$ffi_deps" | grep -E "$net_crates" || true)
if [ -n "$bad" ]; then
    echo "FAIL: networking-capable dependencies in the phone app's library:"; echo "$bad"; exit 1
fi
ffi_lic=$(cargo tree -p seedlock-ffi --edges normal,build --target all --prefix none --format '{p} | {l}' --locked) || { echo "FAIL: cargo tree did not work, so the phone app's licenses cannot be checked."; exit 1; }
ffi_lic=$(printf '%s\n' "$ffi_lic" | sed 's/ (\*)$//' | sort -u)
missing=$(printf '%s\n' "$ffi_lic" | grep -E '\| *$' || true)
if [ -n "$missing" ]; then
    echo "FAIL: these crates of the phone app's library declare no license, so check them by hand:"; echo "$missing"; exit 1
fi
# UniFFI's own crates (uniffi, uniffi_core, uniffi_macros ...) are under MPL-2.0, which applies file by file
uniffi='^uniffi(_[a-z]+)* v[0-9.]+( \(proc-macro\))? \| MPL-2\.0$'
copyleft=$(printf '%s\n' "$ffi_lic" | copyleft_lines | grep -vE "$uniffi" || true)
if [ -n "$copyleft" ]; then
    echo "FAIL: copyleft licenses in the phone app's library (UniFFI's own MPL-2.0 crates excepted), so check these by hand:"; echo "$copyleft"; exit 1
fi
not_mit=$(printf '%s\n' "$ffi_lic" | grep -E '^seedlock-(ffi|qr|core) ' | grep -v '| MIT$' || true)
if [ -n "$not_mit" ]; then
    echo "FAIL: SeedLock's own crates must say license = \"MIT\" (Cargo.toml, [workspace.package]):"; echo "$not_mit"; exit 1
fi
ffi_libs=$(printf '%s\n' "$ffi_lic" | grep -cvE '^seedlock-(ffi|qr|core) ')
mpl=$(printf '%s\n' "$ffi_lic" | grep -cE "$uniffi" || true)
#    the build helper (crates/seedlock-tools: license notices, APK check, program check): no networking crate either
tools_tree=$(cargo tree -p seedlock-tools --edges normal --prefix none --locked) || { echo "FAIL: cargo tree did not work, so the build helper's dependency tree cannot be checked."; exit 1; }
bad=$(printf '%s\n' "$tools_tree" | sed 's/ (.*//; s/ v/ /' | sort -u | grep -E "$net_crates" || true)
if [ -n "$bad" ]; then
    echo "FAIL: networking-capable dependencies in the build helper (crates/seedlock-tools):"; echo "$bad"; exit 1
fi
#    the app around it: its Kotlin code, the permissions it asks Android for, and its libraries
app=android/app
own_kotlin=$(grep -rnE 'java\.net\.|javax\.net\.|HttpURLConnection|Socket\(|okhttp|WebView|android\.net\.http' "$app/src/main/java/org/seedlock/app" || true)
if [ -n "$own_kotlin" ]; then
    echo "FAIL: network API used in the app's Kotlin code:"; echo "$own_kotlin"; exit 1
fi
manifest=$app/src/main/AndroidManifest.xml
permissions=$(grep -E '<uses-permission' "$manifest" | grep -v 'tools:node="remove"' | sed 's/.*android:name="//; s/".*//' | sort | tr '\n' ' ')
if [ "$permissions" != "android.permission.ACCESS_NETWORK_STATE android.permission.CAMERA " ]; then
    echo "FAIL: the app's manifest asks for other permissions than the camera and the network state: $permissions"; exit 1
fi
if ! grep -qE '<uses-permission android:name="android\.permission\.INTERNET" tools:node="remove" */>' "$manifest"; then
    echo "FAIL: the app's manifest no longer takes the INTERNET permission out (tools:node=\"remove\")."; exit 1
fi
libraries=$(sed -n '/^dependencies {/,/^}/p' "$app/build.gradle.kts" | grep -vE '^ *(//.*)?$|^dependencies \{$|^\}$' || true)
if [ "$libraries" != '    implementation("net.java.dev.jna:jna:5.18.1@aar")' ] || [ "$(grep -c 'dependencies' "$app/build.gradle.kts")" != 1 ]; then
    echo "FAIL: the app has other libraries than JNA (android/app/build.gradle.kts); check what they do by hand:"; echo "$libraries"; exit 1
fi

echo "unsafe code (terminal and process calls only, never in the core library):"
grep -rnw unsafe crates/*/src | sed 's/^/  /' | cut -c1-110
echo "Programs SeedLock can start (nothing else does):"
grep -rn 'Command::new' crates/*/src | grep -v '^crates/seedlock-tools/' | sed 's/^/  /'
echo "Programs the build helper starts (where the app and the programs are built; never part of SeedLock):"
grep -rn 'Command::new' crates/seedlock-tools/src | sed 's/^/  /'

echo "OK: $count crates in the dependency tree, none with network access; no network API in SeedLock's own source."
echo "OK: all $libs libraries SeedLock is built from (on every system) have permissive licenses, none copyleft; SeedLock itself is MIT."
echo "OK: the phone app's library: $ffi_count crates in its dependency tree (on every system), none with network access; all $ffi_libs libraries it is built from have permissive licenses, except UniFFI's own $mpl crates (MPL-2.0)."
echo "OK: the phone app asks Android only for the camera and the network state (INTERNET is taken out), JNA is its only other library, and its Kotlin code uses no network API."
echo "OK: the build helper (crates/seedlock-tools) has no networking crate either, and starts only cargo and the Linux program it checks."
