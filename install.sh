#!/bin/sh
# Builds SeedLock and installs the program, so that you can start it from any folder by typing:  seedlock
#
#   sh install.sh                     installs into ~/.local/bin
#   sh install.sh /usr/local/bin      installs into another folder (that one may need: sudo sh install.sh /usr/local/bin)
#
# For the computer that builds SeedLock (Linux and macOS). It needs Rust ("cargo", see Part 1 of the README) and, the
# first time, an internet connection to download the libraries (to build fully offline, see docs/DEVELOPMENT.md).
# The offline computer does not need this script: it only needs the program file (see Part 2 of the README).
set -eu

dest="${1:-$HOME/.local/bin}"
case "$dest" in /*) ;; *) dest="$(pwd)/$dest" ;; esac    # a relative folder is relative to where you typed the command
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    cat >&2 <<'MSG'
SeedLock is built with Rust, and "cargo" was not found on this computer. Install Rust first (README, Part 1):
  1. Linux: install a C compiler and curl
       Debian, Ubuntu : sudo apt install build-essential curl
       Arch           : sudo pacman -S --needed base-devel curl
       Fedora         : sudo dnf install gcc curl
     macOS: xcode-select --install
  2. Install Rust with its official installer, and press Enter for the standard installation:
       curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  3. Close the terminal, open a new one, and run this script again.
(If you just installed Rust, opening a new terminal may be all that is missing.)
MSG
    exit 1
fi

# SeedLock needs Rust 1.85 or newer (Debian 13 has exactly that). Older ones stop with a less clear error.
version=$(cargo --version 2>/dev/null | awk '{print $2}')
major=${version%%.*}; rest=${version#*.}; minor=${rest%%.*}
case "$major$minor" in *[!0-9]*|"") ;; *)
    if [ "$major" -lt 1 ] || { [ "$major" -eq 1 ] && [ "$minor" -lt 85 ]; }; then
        cat >&2 <<MSG
SeedLock needs Rust 1.85 or newer, and this computer has $version. Debian 12 and other older systems
install an older one with apt. Install the current Rust with its official installer instead (one command,
no root needed; README, Part 1):
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
then open a new terminal and run this script again.
MSG
        exit 1
    fi ;;
esac

echo "Building SeedLock (a few minutes the first time) ..."
cargo build --release --locked -p seedlock-cli

mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
install -m 755 target/release/seedlock "$dest/seedlock"

echo
echo "Installed: $dest/seedlock"
if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum "$dest/seedlock" | cut -d' ' -f1); how="sha256sum seedlock"
elif command -v shasum >/dev/null 2>&1; then
    sum=$(shasum -a 256 "$dest/seedlock" | cut -d' ' -f1); how="shasum -a 256 seedlock"
else
    sum=""; how=""
fi
if [ -n "$sum" ]; then
    echo "SHA-256 : $sum"
    echo "If you copy the file to another computer, run  $how  there: the number must be the same."
fi
echo
case ":$PATH:" in
    *":$dest:"*) echo "Start it by typing:  seedlock" ;;
    *)
        echo "Start it with its full name:  $dest/seedlock"
        echo "or, to just type  seedlock , add this line to ~/.zshrc (macOS) or ~/.bashrc (Linux) and open a new terminal:"
        echo "  export PATH=\"$dest:\$PATH\""
        ;;
esac
echo "SeedLock keeps your backups in a folder named \"seedlocker backups\" inside the folder you start it from."
