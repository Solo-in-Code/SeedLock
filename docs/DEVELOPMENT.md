# SeedLock for developers

This page is for people who want to build SeedLock in other ways, change its code, or check how it was tested. To
build SeedLock and use it, the [README](../README.md) and the [user guide](GUIDE.md) are all you need.

SeedLock is written in Rust: memory-safe, with secrets wiped from memory, and built into one small program file. Its
core library, `seedlock-core`, does no input or output of its own (no files, no terminal, no camera), so that it can
later be built into Android and iOS apps unchanged.

## Contents

1. [Building](#building)
2. [Making a release](#making-a-release)
3. [Supply chain and the offline audit](#supply-chain-and-the-offline-audit)
4. [How SeedLock was verified](#how-seedlock-was-verified)
5. [Technical notes](#technical-notes)
6. [Code layout](#code-layout)
7. [Limitations and roadmap](#limitations-and-roadmap)

## Building

### What you need

* **Rust 1.85 or newer** (`rust-version` in `Cargo.toml`). SeedLock is tested with Rust 1.91, and also built with
  1.85. The README's [Part 1, Way B](../README.md#way-b-build-it-yourself) installs it with rustup, the official
  installer, which works on every system. Packaged versions work too when they are new enough: on Debian 13,
  `sudo apt install cargo` gives exactly 1.85; Debian 12 and other older systems install an older one, so use rustup
  there (`install.sh` checks the version and says so); Arch's `rust` package and Homebrew's `rust` are current.
* **A C compiler.** `secp256k1-sys`, the library under rust-bitcoin's signatures, compiles the C code of
  libsecp256k1 (through the `cc` crate). On Linux that is `gcc` (`build-essential`, `base-devel`), on macOS Apple's
  command line tools, on Windows the Visual Studio C++ build tools, which rustup-init offers to install.
* **To build on a computer that is never online,** Rust itself can be installed without internet with its
  standalone installers (see [other installation methods](https://forge.rust-lang.org/infra/other-installation-methods.html)),
  and the libraries can be brought along as described in [Building fully offline](#building-fully-offline).

### Commands

```sh
cargo build --release --locked                   # -> target/release/seedlock (about 4 MB)
cargo run --release -- sign "my wallet" --scan   # build and run; SeedLock's own options go after the --
cargo test --workspace --locked                  # the automated tests
```

`--locked` makes Cargo use exactly the library versions in `Cargo.lock` and stop if they would change. The release
build uses link-time optimization, one code generation unit, `panic = "abort"` (a crash never unwinds, so it cannot
leave half-cleaned secrets behind in unwinding frames), strips symbols and keeps integer overflow checks on. Debug
builds and the tests compile the libraries with optimization, because Argon2 with hundreds of MiB is painfully slow
without it.

### Installing it on the computer that builds it

The README copies the program file to the offline computer by hand. On the computer that builds it (or on a single
computer that is both, before it goes offline for good), SeedLock can also be installed, so that typing `seedlock`
starts it from any folder:

```sh
sh install.sh                    # builds it, then copies it to ~/.local/bin
sh install.sh /usr/local/bin     # another folder (that one may need: sudo sh install.sh /usr/local/bin)
cargo install --path crates/seedlock-cli --locked    # or Cargo's own way: installs into ~/.cargo/bin
```

`install.sh` (Linux and macOS) checks that Cargo is there and new enough, builds, installs, prints the program's
SHA-256 checksum, and says how to add the folder to the `PATH` when it is not there yet. It needs Cargo, so it cannot
run on an offline computer that only has the program file. An installed SeedLock still keeps its backups in the
`seedlocker backups` folder of the folder you are in when you start it.

### Building fully offline

When the computer that builds SeedLock must never go online (for example, the offline computer itself), download the
libraries' source code on an online computer first:

```sh
# online, in SeedLock's folder:
cargo vendor vendor
printf '[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n' >> .cargo/config.toml
# copy the whole folder, with vendor/ and .cargo/, to the offline computer; there:
cargo build --release --locked --offline
```

The `>>` adds these lines to `.cargo/config.toml`, which already holds the Windows setting described below.

### Which computers a build runs on

A program file runs on the kind of computer it was built for: the same system (Linux, macOS, Windows) and the same
kind of processor (x86-64, Apple silicon, ARM). A Linux build runs on other Linux computers of the same processor kind
with the same or a newer system (it needs the C library, glibc, at least as new as on the computer that built it:
otherwise it stops with `GLIBC_2.xx not found`). So build on the oldest system you want it to run on, or on that
computer itself.

The ready-made programs of a release avoid these limits where they can (see [Making a release](#making-a-release)):
the Linux one is a static program built with musl, so it needs no glibc and runs on any x86-64 Linux; the macOS one
holds both kinds of Mac processor; and every Windows build has the C runtime inside the program, by the setting in
`.cargo/config.toml` (`-C target-feature=+crt-static`), so it also starts on a computer without Microsoft's "Visual
C++ Redistributable", where it would otherwise stop with `VCRUNTIME140.dll was not found`. A Windows build without that
setting (the file missing, or a `RUSTFLAGS` variable that replaces it) stops with a message saying so (`main.rs`).

Two builds of the same source are usually not identical byte for byte when they are made on different computers
(Rust records file paths and its own version in the program). That is why the README compares the copy on the
offline computer with the file you built yourself, by its SHA-256 checksum.

### The version number

The program takes its version from one place, `version` under `[workspace.package]` in the top `Cargo.toml`. The
menu's title shows it (`=== SeedLock 0.1.2: offline Bitcoin seed tool ===`), and `seedlock --version` prints it.
`Cargo.lock` records it too: after changing it, run `cargo build --offline` once without `--locked`, so that
`Cargo.lock` follows. The README and the guides show it as well; see [Making a release](#making-a-release).

### The Windows console code

The Windows console code (the live dice counter, colours and cursor moves in the console window) is compiled only on
Windows. On Linux it is type-checked with `RUSTFLAGS="--cfg seedlock_check_windows" cargo check`, since it cannot
run there.

## Making a release

A release is a version of SeedLock that people download exactly as it was tested, from the repository's
**Releases** page: the ready-made programs (README, Part 1, Way A) and the source code (Way B).

1. **The version.** Set the new one in `Cargo.toml` (`version` under `[workspace.package]`, for example `0.1.3`) and
   run `cargo build --offline` once without `--locked`, so that `Cargo.lock` follows. Then replace the old number
   with the new one in `README.md`, `docs/GUIDE.md` and this page (search and replace): the status note, the program
   and folder names in Part 1, and the menu. `cargo test` fails if one was forgotten (the test
   `the_readme_and_the_guides_show_this_version`).
2. **Check everything:** `cargo test --workspace --locked`, the three test scripts, `sh tools/audit-offline.sh` (see
   [Re-running the tests](#re-running-the-tests)), and a build with Rust 1.85, the oldest version SeedLock supports.
3. **Put the files on GitHub,** on the `main` branch. In the web interface: open the folder a changed file belongs
   in, click **Add file > Upload files**, and drag the file in (a file with the same name is replaced). Upload the
   files themselves, never a zip file: GitHub does not unpack it. Most file managers hide files and folders whose
   names start with a dot, such as `.cargo/config.toml`: create those with **Add file > Create new file** instead,
   typing the whole path as the name (`.cargo/config.toml`) and pasting the content (copy it from the file itself,
   opened in a text editor, so that no spaces get in front of the lines). Then check that every file is in its
   folder: a file uploaded on the repository's main page lands at the top. To remove a file from GitHub, open it,
   click **...** at the top right, then **Delete file** and **Commit changes**.
4. **Build the programs** from exactly those files: on each computer, download them from GitHub (**Code > Download
   ZIP**), then, in that folder:

   | System | Commands | The program |
   |---|---|---|
   | Linux (x86-64) | `sh tools/build-release.sh` | `dist/seedlock-X.Y.Z-linux-x86_64` |
   | macOS | `sh tools/build-release.sh` | `dist/seedlock-X.Y.Z-macos` |
   | Windows | the four commands below, in PowerShell | `seedlock-X.Y.Z-windows-x86_64.exe` |

   ```powershell
   rustup target add x86_64-pc-windows-msvc
   cargo build --release --locked --target x86_64-pc-windows-msvc --config "target.x86_64-pc-windows-msvc.rustflags=['--remap-path-prefix=$env:USERPROFILE=~']"
   Copy-Item target\x86_64-pc-windows-msvc\release\seedlock.exe seedlock-X.Y.Z-windows-x86_64.exe
   [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes("$PWD\seedlock-X.Y.Z-windows-x86_64.exe")).Contains($env:USERPROFILE)
   ```

   `X.Y.Z` is the version. The script needs Rust installed with rustup, and on Linux musl; it says how to install
   what is missing. It prints each program's SHA-256; on Windows,
   `(Get-FileHash seedlock-X.Y.Z-windows-x86_64.exe).Hash` shows it. Write them down. (Naming the Windows target makes
   an x86-64 program even on a Windows computer with an ARM processor, where Rust would otherwise build for ARM.)

   **Your user name stays out of the programs.** Rust writes the full path of each library's source files into the
   program, for its error messages, and that path holds the user name of the computer that builds it:
   `/home/NAME/.cargo/...`, `/Users/NAME/.cargo/...` or `C:\Users\NAME\.cargo\...`. The script, and the `--config`
   part of the Windows command, make Rust write `~` in place of the home folder (`--remap-path-prefix`; on Windows it
   is added to the setting in `.cargo/config.toml`, which stays in force). The script then checks that the program
   no longer contains the home folder's path; on Windows the last command does that, and must print `False`.
5. **Try each program** on its system: it starts, shows the version in the menu's title, and opens a backup. Best on
   a computer where Rust was never installed, as the users' computers are. On Linux, the three test scripts also take
   the ready-made program: give its path instead of `target/release/seedlock`.
6. **The checksum list.** Bring the three programs to one Linux or Mac computer (a USB stick is fine), into one
   folder, and check that each one still has the SHA-256 written down in step 4. Then, in that folder:

   ```sh
   sha256sum seedlock-X.Y.Z-* > SHA256SUMS.txt       # on a Mac: shasum -a 256 seedlock-X.Y.Z-* > SHA256SUMS.txt
   gpg --local-user E43521D944AE9BAA9FB2D11D08B65FB65CB1D806 --armor --detach-sign SHA256SUMS.txt
   gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt    # must say: Good signature, with the key's fingerprint
   ```

   The second line signs the list with SeedLock's release key (see [The release key](#the-release-key)) and writes
   the signature, `SHA256SUMS.txt.asc`.

7. **Create the release.** On the repository's main page, click **Releases** on the right, then **Draft a new
   release** (before the first release, the link is called **Create a new release**). Open **Choose a tag**, type
   the version with a `v` in front (for example `v0.1.3`) and click **Create new tag**; **Target**: `main`.
   **Release title**: `SeedLock` and the version. In **Describe this release**, say what changed, and that SeedLock
   is not independently audited. Drag the three programs, `SHA256SUMS.txt` and `SHA256SUMS.txt.asc` into the box for
   binaries below it, and wait until all of them are listed. Leave **This is a pre-release** off and **Set as latest
   release** on, so that this release is the newest one, and click **Publish release** (or **Save draft**, to finish
   later). GitHub adds the source code of the tag by itself, as **Source code (zip)** and **Source code (tar.gz)**.

Change nothing on GitHub between step 3 and step 7: the tag that step 7 creates must hold exactly the files the
programs were built from.

**Why a checksum list, and why signed.** With `SHA256SUMS.txt`, users check that the program they downloaded is
complete and is the file the list names, and the same number then checks the copy on their offline computer. But
whoever could change the release could change the list too. A signature made with the release key shows that the list
comes from the holder of that key, as long as people can compare the key's fingerprint with one published outside
GitHub. Do not put checksums of GitHub's own source code archives in the list: GitHub generates them on request,
caches them for a while and then deletes them
([GitHub's documentation](https://docs.github.com/en/repositories/working-with-files/using-files/downloading-source-code-archives)),
so their exact bytes are not something to promise. Whatever else, protect the GitHub account itself with two-factor
sign-in, best with a passkey or a security key.

**The programs are not signed by Apple or Microsoft** (that needs their paid developer programs), so the first time a
downloaded program starts, macOS says that Apple could not verify it and Windows shows "Windows protected your PC". The
README (Part 3) says what to do.

### The release key

SeedLock's release key signs the checksum lists. Its fingerprint, which the README and the guide show too:

```
E435 21D9 44AE 9BAA 9FB2  D11D 08B6 5FB6 5CB1 D806
```

It was made on 10 October 2026 and expires on 9 October 2028. Before then, extend it by two more years, the key and
its subkey:

```sh
gpg --quick-set-expire E43521D944AE9BAA9FB2D11D08B65FB65CB1D806 2y
gpg --quick-set-expire E43521D944AE9BAA9FB2D11D08B65FB65CB1D806 2y '*'
```

Then export it again into `docs/release-key.asc` (below) and upload it to the repository and to keys.openpgp.org.
Signatures made before the key expires keep checking as good.

The public key is in the repository as [`docs/release-key.asc`](release-key.asc), and on
[keys.openpgp.org](https://keys.openpgp.org). In SeedLock's folder, this writes it:

```sh
gpg --armor --export E43521D944AE9BAA9FB2D11D08B65FB65CB1D806 > docs/release-key.asc
```

If the key ever has to be replaced, make a new one the same way:

1. `gpg --full-generate-key`: press Enter for the default kind of key (ECC with Curve 25519 in GnuPG 2.4; older
   versions offer RSA, also fine), then give an expiry date (for example `2y`, two years; it can be extended later),
   the name "SeedLock releases", an email address for SeedLock, and a strong passphrase.
2. `gpg --fingerprint EMAIL` shows the new key's fingerprint, 40 letters and digits in groups of four.
3. `gpg --armor --export EMAIL > docs/release-key.asc` writes the public key; put that file in the repository.
4. Publish the same public key outside GitHub, so that users have something to compare with: upload
   `docs/release-key.asc` at [keys.openpgp.org](https://keys.openpgp.org/upload), click **Send Verification Email**
   on the page that follows, and confirm with the link in that email (until then, the key server does not publish the
   key's address). If you have a website or a profile elsewhere, show the fingerprint there too.
5. Put the new fingerprint everywhere this one is: the README (Part 1, Way A), the guide (Checking a downloaded
   program), and this page, also in the signing command above. `cargo test` fails if they are not all the same (the
   test `the_release_key_fingerprint_is_the_same_everywhere`).
6. Keep a copy of the secret key in a safe place, apart from the computer, for example on an encrypted USB stick.
   Write it straight onto the stick, not into SeedLock's folder, from where it could end up uploaded with the other
   files: in a terminal, type `cd `, drag the stick onto the window and press Enter, then:

   ```sh
   gpg --armor --export-secret-keys EMAIL > seedlock-release-key-SECRET.asc
   cp ~/.gnupg/openpgp-revocs.d/*.rev .
   ```

   The second line copies the revocation certificate that GnuPG made with the key: if the key is ever lost or
   stolen, it lets you tell everyone not to trust it any more.

## Supply chain and the offline audit

`Cargo.lock` is included and pins every library: 81 crates in the program's dependency tree, none of which can use
the network. Build with `--locked`, review `Cargo.lock`, and, if you like, vendor the sources (above) and read what
you build. There is no telemetry and no network code: the online check only reads the computer's own routing table.

`sh tools/audit-offline.sh` checks, on every run:

1. no networking, HTTP, TLS or async-runtime crate anywhere in the dependency tree;
2. no networking API in SeedLock's own source code;
3. the only other programs SeedLock can start, and only from the places listed: ffmpeg and ffplay for the camera,
   ffplay for the QR window, `/sbin/route` on macOS and `route.exe print` on Windows (to see whether the computer is
   online: they read the routing table and send nothing), `reg.exe query` on Windows and `/usr/sbin/sysctl` on macOS
   (to see whether it is a virtual machine: they read its firmware description), and, only with `--qr-browser`, the
   system's "open" command (`xdg-open`, `open` or `explorer`); never from the core library;
4. `unsafe` code only in `ui.rs` and `main.rs`, and never in the core library, which forbids it;
5. the licenses (below).

It prints every `unsafe` line and every place a program is started, so they can be read in one go. Run it after every
dependency update. It needs Cargo; if Cargo cannot run, it says FAIL, never OK.

**Unsafe code.** The command-line program has a handful of `unsafe` lines for terminal and process calls only: in
`ui.rs` (the window size; reading single key presses for the dice counter; putting the terminal back after Ctrl+C at
a hidden prompt; and on Windows the console calls that do the same there and let the console window understand
colours and cursor moves) and in `main.rs` (switching off core dumps on Linux and macOS, so that a crash cannot write
secrets to disk).

### Licenses of the libraries

SeedLock itself is under the MIT license (`license = "MIT"` in `Cargo.toml`, and the `LICENSE` file). The logo is
not: see [`logo/README.md`](logo/README.md).

The 87 libraries SeedLock can be built from (on every system, build helpers included) keep their own licenses, all
of them permissive: most are MIT or Apache-2.0, the rust-bitcoin libraries are public domain (CC0), and a few are BSD,
ISC, Zlib, Unicode or BlueOak. None is copyleft (GPL and the like, which would require SeedLock to use that license
too); `sh tools/audit-offline.sh` checks this on every run. Libraries used only by the tests never reach the program.
If you publish built programs and not only the source, include those libraries' license notices with them
([`cargo about`](https://github.com/EmbarkStudios/cargo-about) can collect them).

## How SeedLock was verified

### Automated tests

**129 automated tests** (`cargo test --workspace --locked`):

* **68 for the core library:** backup tamper detection on every bit; BIP39/32/44/49/84/86 test vectors; BIP380
  descriptor checksums; PSBT signing with signature verification for SegWit v0 and Taproot; the swapped-change attack;
  refusal cases; animated-QR round trips, and the scanner that collects QR pieces in any order, with repeats, and
  starts over when another transaction appears. Multisig: Sparrow's descriptor forms read and wrong ones refused;
  sorted 2-of-3 scripts; signing one cosigner at a time with the signature checked; refusal when unregistered or when
  the script was changed; the swapped multisig change; nested and legacy wallets and unsorted keys, each signature
  checked over its own signature hash; a legacy input without its previous transaction refused; the UR crypto-output
  format read back, both as urtypes writes it and exactly as Sparrow writes it ("is-private: false" on every key,
  labels), and private or master keys refused; a cosigner key written as a crypto-account; the descriptor scanner
  taking animated parts in any order; and a small strict CBOR reader. BBQr: codes made by Coinkite's reference code
  read in all three encodings, in any order and with repeats; a code that would unpack to more than 16 MB refused; a
  BBQr transaction and a BBQr descriptor taken by the scanners, and a finished transaction passed over.
* **35 unit tests of the command-line program:** a PNG encoder read back by a decoder written for the test; the web
  page; reading QR codes in soft and noisy pictures and saying where they are; dense codes photographed on a screen
  (pixel pattern, glare, soft focus) that only the light-evening tries read; a code in view that cannot be read,
  noticed as such; the outline and progress bar drawn on the preview; a whole transaction rebuilt from pictures; the
  pipes to stand-in ffmpeg and ffplay programs; a preview that fails or is missing; both terminal QR styles, with the
  full one read back square by square and decoded; which terminal gets which style; the QR window fed through a pipe
  to a stand-in ffplay (every code in turn, each for its whole interval, and a readable QR code) and a window that
  cannot be opened; a backup read from its QR code among other codes, while a damaged one is never taken; dropped file
  paths in each terminal's format; the network found from the key paths; the online check reading Linux routing
  tables, macOS `route` output and Windows `route print` output in English and Spanish; a virtual machine recognised
  from its firmware description (and real computers not); Windows registry values; a scanned backup matched to its
  file; and which files of an earlier version go into the cosigner keys folder.
* **26 end-to-end tests of the real program:** among them `sign --scan` through a stand-in ffmpeg; signing without
  `--network` (a test-network transaction is recognised and shown with testnet addresses, a mainnet one is called real
  bitcoin, `--network` still wins, and a transaction with no key paths gets a clear error instead of a guess); a
  registered 2-of-3 wallet signed by two cosigners in turn, refused when unregistered, with the files an earlier
  version kept next to the backups moved into their folders first; the version in the menu's title, above the
  menu, and from `--version`; the same version everywhere the README and the guides give it; and the same release key
  fingerprint everywhere they give it, which is also the fingerprint of the key in `docs/release-key.asc`.

### Independent implementations agree

All re-runnable, see `tools/`:

* `recover.py` (argon2-cffi and libsodium) decrypts what SeedLock encrypts, for 12 to 24 words and ASCII and Unicode
  passwords, and a known-answer vector is pinned.
* `crosscheck.py` compares fingerprints, xpubs, descriptors and hundreds of addresses on random seeds (with and without
  passphrases; all four address types; mainnet and testnet) with `bip-utils` and `embit`.
* `crosscheck_psbt.py` has `embit` build a PSBT, SeedLock sign it, and `embit` verify the signature.
* `crosscheck_backup.py` compares `--dice-only` with SeedSigner's `sha256(rolls)` method.

### The interactive experience

`tools/interactive_test.py` drives the real program in a pseudo-terminal: hidden input; the quiz; the generated
password; the menu and its wording; the backups folder and the name prompt (including a taken name); choosing a
backup by number, by name and by dropping the file (as Linux terminals, Apple's Terminal and `file://` addresses paste
it); the online warning before the menu (a boxed warning naming the connection, then a reminder; offline said once; an
unreadable routing table said plainly; the virtual machine warning); the live dice counter (it moves with every key; a
non-die, Backspace, an arrow key, changing the last roll); Ctrl+C at a hidden prompt, after which the terminal shows
what you type again; that no `--force` text appears anywhere; that typed secrets are never echoed; a window that is
too small; the QR code drawn in the terminal (captured from an emulated screen, decoded with OpenCV and reassembled by
Foundation Devices' reference UR library into exactly the signed PSBT, using the fixed, theme-proof colour codes and
wrapped in synchronized output); and Apple's Terminal (with `TERM_PROGRAM` set the way that app sets it): the boxed
note, then every frame of the animated code, the static code and a backup's code in the full style, read back from the
raw output (no block characters; every square two spaces of one colour), decoded, and reassembled into exactly the
signed PSBT or the backup text; with it unset, the compact style and no note.

### The QR and camera flows

`tools/qr_flow_test.py` tests them end to end with independent tools:

* menu 8's PNG is read back by OpenCV and equals the backup text;
* menu 6's explanations, file list, wrong-path handling and the network found by itself are driven in a
  pseudo-terminal;
* a backup is read from its QR code through the **real ffmpeg** (a soft, noisy, compressed video that shows a
  transaction's code first, which is passed over): it opens (menu 4) and signs (menu 6, the signature verified by
  `embit`); menu 8's copy goes to a dropped folder;
* menu 5's fingerprint, `m/84'/...` derivation, xpubs of all four address types and first address equal `embit`'s,
  on mainnet (found from the wallet's own transaction) and on a test network (after the one question); its descriptor
  checksum agrees with BIP380's reference code, and the saved QR picture reads back as the descriptor;
* the optional browser page is checked by rasterising every QR picture on it and rebuilding the signed PSBT with the
  Foundation UR library, by running its script in a real DOM (jsdom; a negative control proves that the check catches
  the bug an earlier version had, where a script cannot change the class of an SVG element) and by simulating its
  CSS-only fallback;
* camera scanning runs through the **real ffmpeg**, playing an H.264-compressed video, made by an independent encoder
  (Foundation UR and `segno`), of a 12-part animated code that is blurred and noisy like a webcam picture of a screen:
  SeedLock scans it, shows the review and signs, and `embit` verifies the signature;
* the preview window is the **real ffplay** on a virtual X screen: a screenshot of it is decoded back to the QR code,
  and the window closes when the scan is done;
* the QR window is also the **real ffplay** on a virtual X screen: a film of the screen is decoded picture by picture,
  the parts come in order and are reassembled by the Foundation UR library into exactly the signed PSBT, a backup's
  code reads back as exactly the backup text, typing `t` switches to the terminal, Enter closes the window, and
  without a screen or without ffplay SeedLock says why and draws the code in the terminal.

The QR reader was chosen by measurement on simulated webcam pictures (108 pictures with blur, noise, tilt and small
size): trying each picture as it is reads 50% of them; trying it also enlarged, sharpened and reduced reads 75%
(OpenCV's own decoders: 43% and 54%); on the realistic ones (code at least 260 pixels wide, light blur), 100% against
81%. Animated codes need only some of the pictures. On test photos of dense codes on a screen, the light-evening
tries raised the score from 29 to 48 of 64.

### Multisig, checked independently

`tools/multisig_test.py`: `embit`, another Bitcoin library, derives the three cosigners' keys, writes the wallet's
descriptor the way Sparrow exports it (checksum by BIP380's reference code) and builds the transactions; SeedLock only
shows keys, registers and signs.

* The cosigner key SeedLock shows equals embit's. Registering from the exported file (dropped in) shows the same first
  address as embit (by hand and by embit's descriptor engine), and a changed checksum is refused.
* Cosigner A signs in menu 6 and cosigner B on the command line, and embit verifies both signatures over its own
  BIP143 hash and orders them for `OP_CHECKMULTISIG`. A swapped multisig change is shown as leaving the wallet.
* The cosigner key's QR code is read with `urtypes`, the library SeedSigner uses for these codes, and its bytes equal
  what urtypes itself writes.
* A nested wallet is registered by scanning an animated `crypto-output` code written exactly the way Sparrow's source
  writes it (its SettingsController and hummingbird library: "is-private: false", coin info, depth, a label on each
  key, keys in Sparrow's order), through the real ffmpeg. A legacy wallet with its keys in a fixed order (m/45') is
  registered from a file. For both, two cosigners sign and embit verifies the signatures over its own signature
  hashes.
* Menu 10, 1 with the backup read from its QR code (through the real ffmpeg) names the keystore after the backup's
  file, found in the backups folder; for a backup kept elsewhere it asks for the name.
* Codes in Coinkite's BBQr format, made by Coinkite's own reference code: a wallet registered from its descriptor as a
  compressed BBQr series, and its transaction scanned as a compressed BBQr series and signed; embit verifies.
* A whole wallet in one static QR code, as Sparrow shows it at medium density (UR and BBQr), drawn the way Sparrow
  draws it and photographed on a screen: registered through the real ffmpeg (the BBQr photo is one the reader could
  not read before its light-evening tries). While a damaged copy is in view, SeedLock says it sees a code it cannot
  read, and what helps.

### Confirmed on real computers

By SeedLock's users:

* SeedLock builds and runs on **Arch Linux, Debian, Ubuntu/Lubuntu, macOS (Tahoe 26.6.2) and Windows**.
* **Arch Linux:** the camera test with its preview window (menu 9); scanning Sparrow's animated code with the camera;
  the live dice counter; and the animated code drawn in the terminal read by Sparrow, with the transaction signed and
  finished there. The camera delivers 1280 x 720 pictures at about 9 per second, and `xdg-open` opened the web page
  in the browser.
* **macOS:** the camera reads Sparrow's animated code, and in iTerm2 the animated code drawn by SeedLock is read by
  Sparrow and the transaction was signed. In Apple's Terminal the compact code could not be read (see
  [the terminal notes](#drawing-qr-codes-in-the-terminal)).
* **The QR window** (the default) on Arch Linux, Lubuntu and macOS Tahoe 26.6.2 (including in Apple's Terminal):
  Sparrow read the code from it and the transaction was signed.
* **Windows:** the live dice counter; the online warning in a virtual machine (the virtual machine warning while its
  network adapter is on, offline once it is off).
* Also confirmed: reading a backup from its QR code; the online warning on Linux; and multisig with Sparrow (2 of 3,
  native SegWit): the keys typed in or scanned by Sparrow's xpub button, the wallet's descriptor from a file, from an
  animated UR code and from static BBQr codes at two densities (in good light), and its transactions signed by one
  cosigner after the other.

### Not yet confirmed

You should know these:

1. The full style that SeedLock uses in Apple's Terminal (only when a code is drawn in the terminal; the QR window,
   the default, works there) has been checked here by reading it back and decoding it, not on a real Mac.
2. On Windows, the camera (ffmpeg's `dshow` options) and starting the browser with `explorer`; on macOS, the camera's
   preview window and starting the browser with `open`: written and tested here where possible, not yet reported on.
3. The optional web page (`--qr-browser`) has not been looked at in a real browser since its animation was fixed.
4. Menu 5's layout and its QR code for Sparrow; the keystore named after a backup read from its QR code (menu 10, 1);
   the macOS online check (`route`) and virtual machine check (`sysctl`).
5. Nested and legacy multisig, and one large static UR code: checked here only (with urtypes and embit, and with
   photos of Sparrow's exact codes on a screen).

Menu 9 and `seedlock camera` let anyone check their own camera setup.

### Re-running the tests

```sh
cargo test --workspace --locked
pip install pexpect opencv-python-headless numpy cbor2 embit mnemonic pyte segno urtypes
git clone https://github.com/Foundation-Devices/foundation-ur-py
git clone https://github.com/coinkite/BBQr
pip install pyqrcode            # for BBQr's reference code; where it fails (recent Ubuntu): pip install --use-pep517 pyqrcode
npm install jsdom               # in the folder you run the tests from, or set JSDOM_DIR to its node_modules
cargo build --release --locked
python3 tools/interactive_test.py target/release/seedlock --ur-lib foundation-ur-py
python3 tools/qr_flow_test.py target/release/seedlock --ur-lib foundation-ur-py
python3 tools/multisig_test.py target/release/seedlock --ur-lib foundation-ur-py --bbqr-lib BBQr/python
```

They also need ffmpeg (with ffplay), Xvfb (a virtual X screen) and Node.js. The independent cross-checks:

```sh
pip install bip-utils embit mnemonic argon2-cffi pynacl
python3 tools/crosscheck.py --bin target/release/seedlock
python3 tools/crosscheck_psbt.py target/release/seedlock
python3 tools/crosscheck_backup.py target/release/seedlock
python3 tools/recover.py --selftest
```

## Technical notes

### The online check

`netcheck.rs` looks for a way out to other networks, a default route, in the computer's own routing table, without
sending anything: on Linux in `/proc/net/route` and `/proc/net/ipv6_route` (an IPv4 or IPv6 default route that is up,
except the "unreachable" one Linux keeps on the loopback interface), on macOS in the output of
`/sbin/route -n get default` (and `-inet6`), on Windows in the output of `route.exe print`, read in a way that does
not depend on the system's language. The interfaces it goes through are named in the warning.

A virtual machine is recognised from the firmware description it shows: on Linux `/sys/class/dmi/id/sys_vendor` and
`product_name`, on Windows `reg query HKLM\HARDWARE\DESCRIPTION\System\BIOS`, on macOS
`sysctl -n kern.hv_vmm_present`. The tests stand in their own routing tables and firmware descriptions with
`SEEDLOCK_TEST_NET_DIR`.

### Camera scanning

SeedLock reads the camera through ffmpeg, started as a separate program, so that no native camera libraries (hard to
build and to audit) are part of SeedLock: `v4l2` on Linux, `avfoundation` on macOS, `dshow` on Windows. It asks for
1280 x 720 pictures first and falls back to 640 x 480. Each picture is read with `rqrr` as it is, then enlarged,
sharpened and reduced, and then with its light evened out (local mean and contrast), also after a slight blur that
washes out a screen's pixel pattern, within a time budget per picture. Animated UR codes (`ur:crypto-psbt`,
`ur:crypto-output`) are collected in any order (extra parts make up for missed ones); BBQr codes (Coinkite's format:
hexadecimal, base32, or deflate-compressed, which is unpacked to 16 MB at most so that it cannot fill the memory) are
collected the same way. The preview window is an `ffplay` that receives the pictures through a pipe, with an outline
around codes that were read and a progress bar drawn in.

### The QR window

SeedLock draws every QR frame as a picture and sends it through a pipe to `ffplay` (nothing is written to disk), ten
pictures a second, each QR frame for 300 ms. The window is about 640 pixels square and closes when SeedLock stops, for
whatever reason. On Windows, Rust's program search looks in the program's own folder before the `PATH`, which is why
`ffmpeg.exe` and `ffplay.exe` next to `seedlock.exe` are found.

### Drawing QR codes in the terminal

When a code is drawn in the terminal (`t`, `--qr-terminal`, or when no window can be opened):

**Redrawing without tearing.** Every 300 milliseconds the whole code is redrawn (about 30 lines). Each redraw is
wrapped in "synchronized output" (`\x1b[?2026h ... \x1b[?2026l`), a widely supported terminal feature that shows the
redraw only once it is complete, so a phone or webcam taking several pictures a second can never catch the screen
half showing the old code and half the new one. A terminal that has never heard of it ignores those bytes.

**Colour that a theme cannot soften.** The code is always drawn in pure black on pure white, using the fixed entries
of the 256-colour palette rather than the ordinary 8-colour codes. Those 8 are exactly the ones a colour theme can
(and, for a softer look, often does) repaint as an off-black or an off-white, which would quietly lower the contrast
of a code that most needs it. 256-colour support has been standard on every terminal that can show the Unicode
characters used here for well over a decade.

**Apple's own Terminal app gets a larger code.** Found by a person using SeedLock: the usual (compact) code, drawn in
Apple's Terminal, was not read by Sparrow, while the same code in iTerm2 on the same Mac was. A photo of the screen
shows why. Terminal draws the block characters that the compact code is made of (▀ ▄ █, two rows of the code per line
of text) a little shorter than a line, so a thin white line runs through the black parts at the end of every line of
text, that is, after every second row of the code. Measured on the photo, these lines sit at exact whole-line
spacings, and nothing else is wrong. As photographed, neither ZXing-C++ nor OpenCV reads the code. With only those thin
lines filled in (4% of the pixels changed), ZXing reads it, even shrunk to webcam size, and it says exactly what the
screen showed (part 3 of the transaction's 7 pieces, "Frame 3/14"). So in Apple's Terminal, which identifies itself
to every program it starts (`TERM_PROGRAM=Apple_Terminal`), SeedLock draws the **full style** instead: every square of
the code is two spaces with a black or white background, one row of the code per line of text, and no block
characters at all. A terminal paints a background across the whole line; in the same photo, the white border around
the code, drawn exactly that way, is unbroken. The full style is twice as tall and twice as wide (about 107 columns by
58 lines for a typical transaction), so a boxed "IMPORTANT NOTE" explains this first, and the size check asks to
maximize the window and zoom out with Cmd and - until the code fits. `SEEDLOCK_QR_STYLE=full` uses the full style in
any terminal (for example inside tmux or over SSH, where the terminal cannot be recognised), and
`SEEDLOCK_QR_STYLE=compact` turns it off. The two hardenings above (synchronized output, fixed colours) were not the
cause; they stay because they are harmless. Since QR codes open in a window of their own, all of this only matters
when a code is drawn in the terminal.

**The web page is an option, not the default.** Drawing the code in the terminal needs nothing but the terminal, does
not start a browser on the offline computer, and is what has been checked most on real systems. The page
(`--qr-browser`) exists for windows that cannot be made big enough: it scales to the whole browser window.

### The QR formats SeedLock writes

The signed transaction's animated code uses the `ur:crypto-psbt` format that SeedSigner and most hardware wallets
use: the transaction is split into small pictures shown in a loop, and the scanner collects them in any order. A single
static code (`--static-qr`, small transactions only) is available for scripts, but it is much denser than one animated
picture, so it can never be as easy to read. A cosigner key is a `ur:crypto-account` (as SeedSigner writes it), and
menu 5's wallet is the plain output descriptor text, which Sparrow's descriptor import reads.

### Backup format

Documented byte for byte in [`FORMAT.md`](FORMAT.md): Argon2id (256 MiB by default) and XChaCha20-Poly1305, the
header authenticated, a fixed size of 100 bytes, and a `SEEDLOCK1:` + Base32 text form with a checksum that fits in
one QR code. The key-derivation settings (`light` 64 MiB, `standard` 256 MiB, `strong` 1 GiB) are stored inside each
backup. [`tools/recover.py`](../tools/recover.py) is an independent decryptor of about 70 lines, so backups outlive
this program.

## Code layout

```
crates/seedlock-core      the library: mnemonic, entropy, backup format, wallet derivation, multisig, PSBT, QR frames (no I/O)
crates/seedlock-cli       the `seedlock` program: prompts, menu, files, QR pictures and pages, camera scanning, online check
docs/GUIDE.md             the user guide
docs/DEVELOPMENT.md       this page
docs/FORMAT.md            the backup format specification, with a test vector
docs/release-key.asc      the public release key that signs the checksum lists of the releases
docs/logo/                the logo (SVG and PNG files) and make_logo.py, which draws them: all rights reserved, not MIT
.cargo/config.toml        build settings: the C runtime inside the Windows program
install.sh                builds the program and installs it, so that `seedlock` starts it from any folder
LICENSE                   the MIT license (for everything except the logo)
tools/                    recover.py, crosscheck*.py, interactive_test.py, qr_flow_test.py, multisig_test.py, audit-offline.sh,
                          build-release.sh (the ready-made programs of a release)
dist/                     made by build-release.sh: the ready-made programs (never committed)
seedlocker backups/       made where you run seedlock: your backups and their QR pictures (never committed)
seedlocker transactions/  made where you run seedlock: signed files of scanned transactions, and QR pages (never committed)
```

## Limitations and roadmap

Today: the English word list only; multisig as native, nested or legacy SegWit (no Taproot multisig); the unsigned
PSBT comes in as a file or by camera (animated `ur:crypto-psbt`, BBQr, or a plain PSBT in one QR code), and the
signed one leaves as a file or as an animated QR code; a command-line program only.

**Phone.** Backups made today open unchanged on a phone (use `--kdf light` for very old devices). Next steps: wrap
`seedlock-core` with UniFFI (Kotlin/Swift) or a Tauri 2 mobile shell, add camera scanning for backup and PSBT QR
codes (the core already collects animated UR frames, and reading QR codes from camera pictures is in the command-line
crate), then a simple graphical interface. The core has no I/O, so none of this needs a rewrite.

**Reproducible builds.** Today a program that a user builds is not byte for byte the same file as a release's
ready-made program (file paths and the compiler versions are recorded in it). Building the release programs in a fixed
environment (the same Rust and C compiler versions and the same paths, for example in a container) would let anyone
rebuild a release and compare the SHA-256 with `SHA256SUMS.txt`, without having to trust the release key.
