# SeedLock for developers

This page is for people who want to build SeedLock in other ways, change its code, or check how it was tested. To
build SeedLock and use it, the [README](../README.md) and the [user guide](GUIDE.md) are all you need.

SeedLock is written in Rust: memory-safe, with secrets wiped from memory, and built into one small program file. Its
core library, `seedlock-core`, does no input or output of its own (no files, no terminal, no camera), so that it goes
into phone apps unchanged: the Android app, a preview for now, is built on it (see [The phone
app](#the-phone-app-preview)). The releases have it ready-made, signed with SeedLock's own Android key.

## Contents

1. [Building](#building)
2. [The phone app (preview)](#the-phone-app-preview)
3. [Making a release](#making-a-release)
4. [Supply chain and the offline audit](#supply-chain-and-the-offline-audit)
5. [How SeedLock was verified](#how-seedlock-was-verified)
6. [Technical notes](#technical-notes)
7. [Code layout](#code-layout)
8. [Limitations and roadmap](#limitations-and-roadmap)

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
menu's title shows it (`=== SeedLock 0.3.0: offline Bitcoin seed tool ===`), and `seedlock --version` prints it.
`Cargo.lock` records it too: after changing it, run `cargo build --offline` once without `--locked`, so that
`Cargo.lock` follows. The README and the guides show it as well; see [Making a release](#making-a-release).

### The Windows console code

The Windows console code (the live dice counter, colours and cursor moves in the console window) is compiled only on
Windows. On Linux it is type-checked with `RUSTFLAGS="--cfg seedlock_check_windows" cargo check`, since it cannot
run there.

## The phone app (preview)

The Android app is in the folder `android`, and [`android/README.md`](../android/README.md) says how to install the
ready-made app, how to build it, and what to try. It is a preview; from 0.2.1 on it is part of the releases, signed
with SeedLock's own Android key (see [The Android signing key](#the-android-signing-key)). Its tests on a real phone
went well (see [Confirmed on real computers](#confirmed-on-real-computers)). It does what the computer does: it opens
and makes backups (the same files), lays the wallet out for Sparrow, and reads a transaction from Sparrow's animated
QR code with the camera, shows every address and amount, signs, and shows the signed transaction as an animated QR
code. For multisig it does what menu 10 does: it lays a seed's key out for a multisig wallet in Sparrow, keeps the
multisig wallets read from Sparrow's QR code or exported file (as the same `.multisig` files), and signs their
payments as one of their cosigners.

How it is made:

* **`crates/seedlock-ffi`** is the part of SeedLock that the app calls: seeds, backups, the wallet, multisig wallets,
  the review and the signing, QR codes to show, and a scanner that collects QR codes from camera pictures. It is a thin layer over
  `seedlock-core`, with the command-line program's checks and wording. [UniFFI](https://github.com/mozilla/uniffi-rs)
  makes it callable from Kotlin: `crates/uniffi-bindgen` writes the Kotlin code from the library itself (it can write
  Swift too, for an iPhone app later). Seeds stay inside Rust, in memory that is wiped; passwords cross as bytes that
  both sides wipe. Its code is safe Rust only (`#![forbid(unsafe_code)]`). What the phone and the computer both do is
  in `seedlock-core`, for example a backup's QR code that carries the backup's name (`backup::qr_text`,
  [`FORMAT.md`](FORMAT.md), section 6) and the wallet's QR code for Sparrow, a UR `crypto-output` whose key is named
  after the backup, which Sparrow takes as the keystore's label (`Wallet::crypto_output_ur`).
* **`crates/seedlock-qr`** reads QR codes in camera pictures and makes QR codes to show; the command-line program reads
  QR codes with it too (its `scan.rs` brings the pictures from the camera). Both find a transaction's network the same
  way, by its key paths (`psbt::network_hint` in `seedlock-core`).
* **The app** (`android/app/src/main/java/org/seedlock/app`): plain Android classes in Kotlin, without AndroidX or
  any other library except JNA, which the Kotlin code from UniFFI uses to call the Rust library (JNA 5.18.1, which
  also runs on phones with 16 KB memory pages). The camera is read with Camera2 (the grey part of each picture goes to
  the scanner), files are opened and saved with Android's file picker, and backups and multisig wallets are kept in
  the app's own storage.

A plain `cargo build` still builds only the command-line program (`default-members` in `Cargo.toml`); the phone crates
are built with `-p`. `android/build-rust.sh` (on Windows `build-rust.ps1`) builds `libseedlock_ffi.so` with cargo-ndk
for 64-bit and 32-bit ARM phones and the x86-64 emulator, writes the Kotlin code, and writes the license notices that
the app shows (**About SeedLock and its licenses**), with `crates/seedlock-tools` (see [Licenses of the
libraries](#licenses-of-the-libraries)). Android Studio then builds the app, and only the app: the app module has no
instrumented tests, so `app/build.gradle.kts` switches off the separate test app that Android Studio would build next
to it (which has no icon and opens nothing). What Android Studio builds is a **test build**, "SeedLock test"
(`org.seedlock.app.debug`, signed with that computer's own test key), an app of its own beside the released "SeedLock"
(`org.seedlock.app`, signed with SeedLock's key): Android keeps an app to the key it was first signed with, so trying
a build never has to touch the released app or its backups. The library is built with the `phone` profile (in
`Cargo.toml`): the release settings, but with `panic = "unwind"`, because inside an app a panic (a bug) should become
an error the app shows, which UniFFI does, rather than close the app at once. Like the release scripts, the script
keeps your user name out of the library and checks that it did.

What keeps the phone offline and the secrets in it: the app has no INTERNET permission, so Android refuses it every
network connection (the manifest even takes the permission out should a library ask for it); it works only while the
phone is offline (airplane mode on, no network connected): it checks every second while it is in use, its first screen
shows only a message while the phone is online, and going online closes every screen and dialog at once, with any open
seed, lets go of what it waited for, and takes a password it copied off the clipboard (`MainActivity.checkNetwork`);
its screens and its dialogs cannot be captured or recorded (`FLAG_SECURE` on each window); a seed's words cannot be
selected or copied, and a new password reaches the clipboard only when **Copy it** is tapped (for a password manager
on the same phone), marked as sensitive, and SeedLock empties the clipboard when it is back in front if its copy is
still there; its text fields allow no keyboard learning or autofill; its files are left out of Android's cloud backup
and of the copy to a new phone; and leaving the app closes every screen but the first and every dialog, and with them
any open seed (work that ends meanwhile, such as opening a backup, shows nothing). Two exceptions, each for at most 5
minutes, are for a password manager on the same phone: after **Copy it**, the new seed's screen stays open while the
password is pasted into the password manager; and while SeedLock asks for a backup's password, when no seed is open
yet (only the first screen and a backup's or a multisig wallet's screen are under the prompt), the prompt stays while
the password is copied from the password manager, and when the prompt closes, SeedLock empties the clipboard.
`tools/audit-offline.sh` checks the library, the permissions, the app's libraries and its Kotlin code (see [the
audit](#supply-chain-and-the-offline-audit)).

Two more things to know about the app:

* **Answers from Android's file picker wait for the screen.** Up to Android 12, Android hands an app the answer of
  the file picker (and of the camera question) before the app is on the screen again (`onActivityResult` before
  `onStart`: Android 10's `ActivityResultItem` has no lifecycle state of its own; from Android 13 on, for apps that
  target it, the answer comes after `onStart`). The app would then still count itself as left and show nothing, so
  `MainActivity` keeps such an answer until `onStart`. Without this, opening a file did nothing on Android 10 (found
  on the Xiaomi Mi 8, and confirmed fixed there; the emulators with a recent Android do not show it).
* **The fingerprints beside the backups' names.** Once a backup was opened on the phone (or made there), the app
  keeps its wallet's fingerprint without a BIP39 passphrase next to it (`<name>.fingerprint`, in the app's own
  storage; it is not part of the backup's file and goes nowhere with its copies). It shows it beside the backup's
  name, names the backups that are a multisig wallet's cosigners, and says before the password is asked for when a
  transaction is for other wallets than the chosen backup's. Nothing about a passphrase is kept: with one, a seed is
  another wallet, and whether it exists stays the person's secret.

## Making a release

A release is a version of SeedLock that people download exactly as it was tested, from the repository's
**Releases** page: the ready-made programs (README, Part 1, Way A) and the source code (Way B).

1. **The version.** Set the new one in `Cargo.toml` (`version` under `[workspace.package]`, for example `0.2.2`) and
   run `cargo build --offline` once without `--locked`, so that `Cargo.lock` follows. Then replace the old number with
   the new one in `README.md`, `docs/GUIDE.md`, this page and `android/README.md` (search and replace) where they name
   the current version: the status note, the program and folder names in Part 1, the menu and the phone app's file
   name. Where a text says from which version on something is so, its number stays. `cargo test` fails if one was
   forgotten (the test `the_readme_and_the_guides_show_this_version`).
2. **Check everything:** `cargo test --workspace --locked`, the three test scripts (with a build for the tests),
   `sh tools/audit-offline.sh` (see [Re-running the tests](#re-running-the-tests)), and a build with Rust 1.85, the
   oldest version SeedLock supports.
3. **Put the files on GitHub,** on the `main` branch. In the web interface: open the folder a changed file belongs
   in, click **Add file > Upload files**, and drag the file in (a file with the same name is replaced). Many files at
   once: on the repository's main page, drag whole folders (such as `crates` and `docs`) and files together onto the
   upload page; GitHub keeps their folders, replaces the files with the same path and leaves unchanged ones as they
   are (at most 100 files per upload). Take them from a freshly unpacked copy, never from a folder that Cargo or
   Android Studio built in, whose build files (`target`, `android/app/build`, `local.properties` and so on) must not
   go up. Upload the files themselves, never a zip file: GitHub does not unpack it. Most file managers hide files and
   folders whose names start with a dot, such as `.cargo/config.toml`: create those with **Add file > Create new
   file** instead, typing the whole path as the name (`.cargo/config.toml`) and pasting the content (copy it from the
   file itself, opened in a text editor, so that no spaces get in front of the lines); to change one that is already
   there, such as `.gitignore`, open it on GitHub, click the pencil (**Edit this file**), replace its content the
   same way and click **Commit changes**. Then check that every file is in its folder: a file uploaded on the
   repository's main page lands at the top. To remove a file from GitHub, open it, click **...** at the top right,
   then **Delete file** and **Commit changes**.
4. **Build the programs** from exactly those files: on each computer, download them from GitHub (**Code > Download
   ZIP**), then, in that folder:

   | System | Command | The program |
   |---|---|---|
   | Linux (x86-64) | `sh tools/build-release.sh` | `dist/seedlock-X.Y.Z-linux-x86_64` |
   | macOS | `sh tools/build-release.sh` | `dist/seedlock-X.Y.Z-macos` |
   | Windows | `powershell -ExecutionPolicy Bypass -File tools\build-release.ps1` | `dist\seedlock-X.Y.Z-windows-x86_64.exe` |

   `X.Y.Z` is the version. On Windows, type the line in PowerShell or in Command Prompt, both work;
   `-ExecutionPolicy Bypass` lets Windows run this one script, without changing any setting. The scripts need Rust
   installed with rustup, and on Linux musl; they say how to install what is missing. Each one prints the program's
   SHA-256: write them down. (The Windows script makes an x86-64 program even on a Windows computer with an ARM
   processor, where Rust would otherwise build for ARM.)

   **Your user name stays out of the programs.** Rust writes the full path of each library's source files into the
   program, for its error messages, and that path holds the user name of the computer that builds it:
   `/home/NAME/.cargo/...`, `/Users/NAME/.cargo/...` or `C:\Users\NAME\.cargo\...`. The scripts make Rust write `~`
   in place of the home folder (`--remap-path-prefix`; on Windows it is added to the setting in `.cargo/config.toml`,
   which stays in force), then check that the program no longer contains the home folder's path, and stop with a
   message if it does. So build release programs with the scripts, never with `cargo build` alone.

   **And the phone app,** on the computer with Android Studio and SeedLock's Android key, in that downloaded folder
   too (android/README.md, Building it yourself, says what it needs):

   1. Build the Rust part: `sh android/build-rust.sh` (Windows: `powershell -ExecutionPolicy Bypass -File
      android\build-rust.ps1`).
   2. In Android Studio, open the folder `android` of that downloaded folder. Then **Build > Generate Signed App
      Bundle or APK** (in older versions: **Generate Signed Bundle / APK**), choose **APK** and click **Next**. Choose
      SeedLock's key store (**Choose existing...**: the file of [The Android signing key](#the-android-signing-key)),
      type its password, choose the key `seedlock` and type its password, leave **Remember passwords** off; **Next**.
      Choose **release** and click **Create** (or **Finish**). When it is done, a message links to the APK:
      `android/app/release/app-release.apk`.
   3. Check it, in a terminal in that folder: `cargo run --locked -p seedlock-tools -- check-apk
      android/app/release/app-release.apk`. It lists what it checked (the version, a release build that cannot be
      debugged, the permissions, the libraries and the code in it, its license notices, no user name inside, and the
      key it is signed with), puts it into `dist` as `seedlock-X.Y.Z-android.apk` and prints its SHA-256: write that
      down too. If something is wrong, it says what, and nothing goes into `dist`.
5. **Try each program** on its system, offline: it starts, shows the version in the menu's title, and opens a backup.
   Best on a computer where Rust was never installed, as the users' computers are. (The three test scripts cannot
   drive a ready-made program: it has no way to be shown a stand-in network, see [The online check](#the-online-check).)
   Install the phone app on a phone as android/README.md (Installing the ready-made app) says, and open a backup with
   it.
6. **The checksum list.** Bring the three programs and the phone app to one Linux or Mac computer (a USB stick is
   fine), into one folder, and check that each one still has the SHA-256 written down in step 4. Then, in that folder:

   ```sh
   sha256sum seedlock-X.Y.Z-* > SHA256SUMS.txt       # on a Mac: shasum -a 256 seedlock-X.Y.Z-* > SHA256SUMS.txt
   gpg --local-user E43521D944AE9BAA9FB2D11D08B65FB65CB1D806 --armor --detach-sign SHA256SUMS.txt
   gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt    # must say: Good signature, with the key's fingerprint
   ```

   The second line signs the list with SeedLock's release key (see [The release key](#the-release-key)) and writes
   the signature, `SHA256SUMS.txt.asc`.

7. **Create the release.** On the repository's main page, click **Releases** on the right, then **Draft a new
   release** (before the first release, the link is called **Create a new release**). Open **Choose a tag**, type
   the version with a `v` in front (for example `v0.2.2`) and click **Create new tag**; **Target**: `main`.
   **Release title**: `SeedLock` and the version. In **Describe this release**, say what changed, and that SeedLock
   is not independently audited. Drag the three programs, the phone app (`seedlock-X.Y.Z-android.apk`),
   `SHA256SUMS.txt` and `SHA256SUMS.txt.asc` into the box for binaries below it, and wait until all of them are
   listed. Leave **This is a pre-release** off and **Set as latest
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

### The Android signing key

Android installs only signed apps, and it installs an update only when the update is signed with the same key as the
app that is there. So the phone app of every release is signed with SeedLock's own Android key, made once and kept
like the release key:

* if it is **lost**, no update can ever be installed over the app: everyone would have to uninstall it, which deletes
  the backups on their phone, and install the app again;
* if it is **stolen**, someone could sign an app that Android takes as an update of SeedLock's. People would still
  have to install it themselves, and the signed checksum list shows which file is SeedLock's.

It is not the release key that signs the checksum lists: Android uses keys of its own kind.

**Making it** (once), in Android Studio, with SeedLock's `android` folder open:

1. **Build > Generate Signed App Bundle or APK** (in older versions: **Generate Signed Bundle / APK**), choose
   **APK**, click **Next**, then **Create new...** below the key store path.
2. **Key store path:** a folder outside SeedLock's folder (your home folder, for example), and the file name
   `seedlock-android.jks`. Never put it in SeedLock's folder, from where it could end up uploaded with the other
   files: `.gitignore` leaves out `*.jks` files, but GitHub's upload page does not read `.gitignore`.
3. **Password:** a strong one from your password manager, twice. **Alias:** `seedlock`. The key's own **Password:**
   the same one again. **Validity (years):** 50.
4. **Certificate:** in **First and Last Name**, type `SeedLock releases`, and leave the other fields empty. What you
   type here is inside every copy of the app, for everyone to read: never your own name.
5. Click **OK**: the key store is made. Then go on as [Making a release](#making-a-release) says, or close the window.

**Keeping it:** copy `seedlock-android.jks` onto the encrypted USB stick that keeps the release key's secret copy,
and keep its password in your password manager. With those two, any computer with Android Studio can sign the next
release.

**Its fingerprint:** the SHA-256 fingerprint of its certificate is published in
[`android/README.md`](../android/README.md) (Installing the ready-made app) and written into
`crates/seedlock-tools/src/apk.rs` (`RELEASE_CERTIFICATE`), so that `check-apk` refuses an APK signed with any other
key:

```
02:27:2A:CC:06:84:2D:36:3A:19:A6:CF:48:3D:D9:28:5F:CB:CA:C5:38:AB:98:24:51:14:35:2A:B9:68:63:44
```

`seedlock-tools check-apk` shows it, and so does `keytool -list -v -keystore seedlock-android.jks` (the line
`SHA256:`; keytool comes with Android Studio, in its folder `jbr/bin`).

## Supply chain and the offline audit

`Cargo.lock` is included and pins every library: 82 crates in the program's dependency tree, none of which can use
the network. Build with `--locked`, review `Cargo.lock`, and, if you like, vendor the sources (above) and read what
you build. There is no telemetry and no network code: the online check, which keeps SeedLock from working on a
computer that is not offline, only reads the computer's own routing table.

`sh tools/audit-offline.sh` checks, on every run:

1. no networking, HTTP, TLS or async-runtime crate anywhere in the dependency tree;
2. no networking API in SeedLock's own source code;
3. the only other programs SeedLock can start, and only from the places listed: ffmpeg and ffplay for the camera,
   ffplay for the QR window, `/sbin/route` on macOS and `route.exe print` on Windows (to see whether the computer is
   online: they read the routing table and send nothing), `reg.exe query` on Windows and `/usr/sbin/sysctl` on macOS
   (to see whether it is a virtual machine: they read its firmware description), and, only with `--qr-browser`, the
   system's "open" command (`xdg-open`, `open` or `explorer`); never from the core library;
4. `unsafe` code only in `ui.rs` and `main.rs`, and never in the core library, which forbids it;
5. the licenses (below);
6. the phone app: checks 2 to 4 cover its Rust code too; its library has no networking crate either, on every system
   (Android's included, whatever computer runs the audit), and its licenses are checked like the program's; its
   manifest asks Android only for the camera and the network state (to work only while the phone is offline) and takes
   INTERNET out; JNA is its only other library; and its Kotlin code uses no network API;
7. the build helper `crates/seedlock-tools` (the app's license notices, the check of a release APK), which runs on the
   computer that builds the app and is never part of SeedLock: no networking crate in its tree either, and the only
   program it starts is cargo (`cargo tree` and `cargo metadata`, for the notices).

It prints every `unsafe` line and every place a program is started, so they can be read in one go. Run it after every
dependency update. It needs Cargo; if Cargo cannot run, it says FAIL, never OK.

**Unsafe code.** The command-line program has a handful of `unsafe` lines for terminal and process calls only: in
`ui.rs` (the window size; reading single key presses for the dice counter; putting the terminal back after Ctrl+C at
a hidden prompt; and on Windows the console calls that do the same there and let the console window understand
colours and cursor moves) and in `main.rs` (switching off core dumps on Linux and macOS, so that a crash cannot write
secrets to disk). The phone app's crates forbid `unsafe` code; the glue that UniFFI's macros write for the calls from
Kotlin is UniFFI's own.

### Licenses of the libraries

SeedLock itself is under the MIT license (`license = "MIT"` in `Cargo.toml`, and the `LICENSE` file). The logo is
not: see [`logo/README.md`](logo/README.md).

The 88 libraries SeedLock can be built from (on every system, build helpers included) keep their own licenses, all
of them permissive: most are MIT or Apache-2.0, the rust-bitcoin libraries are public domain (CC0), and a few are BSD,
ISC, Zlib, Unicode or BlueOak. None is copyleft (GPL and the like, which would require SeedLock to use that license
too); `sh tools/audit-offline.sh` checks this on every run. Libraries used only by the tests never reach the program.
If you publish built programs and not only the source, include those libraries' license notices with them
([`cargo about`](https://github.com/EmbarkStudios/cargo-about) can collect them).

The phone app brings a few more:

* **UniFFI** (its six crates `uniffi`, `uniffi_core`, `uniffi_macros` and so on) is under the Mozilla Public License
  2.0 (MPL-2.0), a weak copyleft that applies file by file: it covers UniFFI's own files, not SeedLock's, which stay
  MIT. A ready-made APK must say where UniFFI's source code can be had (unchanged, on crates.io and GitHub); the
  Kotlin code that `uniffi-bindgen` writes comes from UniFFI's templates, so it is treated the same way. The audit
  allows MPL-2.0 for UniFFI's own crates and for nothing else.
* **JNA**, which the app uses to call the Rust library, may be used under Apache-2.0 or LGPL-2.1, as one prefers:
  SeedLock takes Apache-2.0. Its native library contains libffi (MIT). The Kotlin standard library inside the app,
  and the JetBrains annotations it uses, are Apache-2.0.

**The app carries the notices** of all its libraries: **About SeedLock and its licenses**, on its first screen, shows
them. `crates/seedlock-tools` writes them (`seedlock-tools notices --android`, run by `android/build-rust.sh` into
`android/app/src/main/assets/notices.txt`) from exactly the libraries the phone library is built with: the crates
`cargo tree` finds for the three kinds of phone processor (without those that only help to build it, but with the
macro crates whose generated code is in it), each with the license it is used under (of the ones it offers, the one
that asks the least), its copyright lines from its own license file, and the license texts: the MIT text once with
every library's copyright lines, the Apache 2.0 and MPL 2.0 texts once, other licenses in full (BSD, ISC, Zlib,
BlueOak, MITNFA), and code inside a crate under a license of its own (the C library libsecp256k1, MIT, inside
`secp256k1-sys`); then Rust's standard library, and the Java part (JNA with libffi, Kotlin's standard library, the
annotations), whose versions it reads from the Gradle files. `check-apk` refuses a release APK whose notices are not
this version's, or whose code is from any other library than those.
* A library that lets you choose between a permissive and a copyleft license is taken under the permissive one; the
  audit accepts it for that reason. So far there is one, `r-efi` (MIT, Apache-2.0 or LGPL), which is only used when
  building for UEFI firmware and never reaches a phone or a computer program.

## How SeedLock was verified

### Automated tests

**167 automated tests** (`cargo test --workspace --locked`):

* **71 for the core library:** backup tamper detection on every bit; a backup's QR code with its name line, still
  read as the backup exactly the way earlier versions read it; BIP39/32/44/49/84/86 test vectors; BIP380 descriptor
  checksums; the wallet for Sparrow as a UR crypto-output named for its keystore, its fields rebuilding BIP84's xpub,
  with each address type's tags and a test network's mark; PSBT signing with signature verification for SegWit v0
  and Taproot; the swapped-change attack;
  refusal cases; animated-QR round trips, and the scanner that collects QR pieces in any order, with repeats, and
  starts over when another transaction appears. Multisig: Sparrow's descriptor forms read and wrong ones refused;
  sorted 2-of-3 scripts; signing one cosigner at a time with the signature checked; refusal when unregistered or when
  the script was changed; the swapped multisig change; nested and legacy wallets and unsorted keys, each signature
  checked over its own signature hash; a legacy input without its previous transaction refused; the UR crypto-output
  format read back, both as urtypes writes it and exactly as Sparrow writes it ("is-private: false" on every key,
  labels), and private or master keys refused; a cosigner key written as a crypto-account; the descriptor scanner
  taking animated parts in any order; and a small strict CBOR reader. BBQr: codes made by Coinkite's reference code
  read in all three encodings, in any order and with repeats; a code that would unpack to more than 16 MB refused; a
  BBQr transaction and a BBQr descriptor taken by the scanners, and a finished transaction passed over. The network
  found from a transaction's key paths (for both programs).
* **32 unit tests of the command-line program:** a PNG encoder read back by a decoder written for the test; the web
  page; the outline and progress bar drawn on the preview; a whole transaction rebuilt from pictures; the pipes to
  stand-in ffmpeg and ffplay programs; a preview that fails or is missing; both terminal QR styles, with the full one
  read back square by square and decoded; which terminal gets which style; the QR window fed through a pipe to a
  stand-in ffplay (every code in turn, each for its whole interval, and a readable QR code) and a window that cannot
  be opened; the programs SeedLock keeps running (the windows, the camera) ended when they are let go of, and all at
  once just before SeedLock stops; a backup read from its QR code among other codes, while a damaged one is never
  taken; dropped file paths in each terminal's format; the network chosen from what the key paths say, `--network`
  winning, and a clear error instead of a guess when they say nothing or contradict each other; the online check
  reading Linux routing tables, macOS `route` output and Windows `route print` output in English and Spanish; a
  virtual machine recognised from its firmware description (and real computers not); Windows registry values; a
  scanned backup matched to its file; and which files of an earlier version go into the cosigner keys folder.
* **31 end-to-end tests of the real program:** among them that SeedLock works only offline (online, and where it
  cannot tell, every command stops before it asks anything, with exit code 3, and so does the menu, while `--help` and
  `--version` still answer); `sign --scan` through a stand-in ffmpeg; signing without `--network` (a test-network
  transaction is recognised and shown with testnet addresses, a mainnet one is called real bitcoin, `--network` still
  wins, and a transaction with no key paths gets a clear error instead of a guess); a registered 2-of-3 wallet signed
  by two cosigners in turn, refused when unregistered, with the files an earlier version kept next to the backups
  moved into their folders first; the backup's QR picture (menu 8) read back as the backup line with the backup's name
  on a line of its own, and as the same backup, while a pasted backup line, which has no name, still gives the line
  alone; a new backup's QR code (`--qr`, as menus 1 and 2 show it) saved as that picture too, and where it is said
  before the code is shown, in menu 8 as well; menu 5's QR picture read back as exactly the UR crypto-output that
  names the keystore after the backup, also written in the saved text file, with the descriptor to paste still shown;
  a backup scanned from a named QR code, through a stand-in ffmpeg, giving its name to the files made from it and to
  the keystore; the version in the menu's title, above the menu, and from `--version`; the same version everywhere the
  README and the guides give it; and the same release key fingerprint everywhere they give it, which is also the
  fingerprint of the key in `docs/release-key.asc`.
* **8 for the build helper** (`seedlock-tools`): license expressions read ("(MIT OR Apache-2.0) AND ISC", the old
  "MIT/Apache-2.0"), and of their choices the license that asks the least taken (MIT before others that ask as
  little); license texts recognised by their wording (and a file that only names licenses not taken for one); an MIT
  text split into its copyright lines and the usual text, also libffi's, with its old quotes, while one with an extra
  condition is kept in full; texts reflowed into paragraphs for the phone; a right release APK passing, its
  certificate read (APK Signature Scheme v3 and v2, manifests with UTF-16 and UTF-8 strings); every mistake found (a
  build that can be debugged, the test build, another version, the INTERNET permission, a missing library or one for
  another processor, code of another library, no notices or another version's, the home folder's path inside,
  Android Studio's test key, no v2 or v3 signature, not an APK at all); the version codes build.gradle.kts gives; and
  the fingerprint of SeedLock's Android certificate the same in the check and in the documents that give it.
* **6 for the QR crate** (`seedlock-qr`, which both programs read QR codes with): soft and noisy camera pictures; dense
  codes photographed on a screen (its pixel pattern, glare, soft focus) that only the light-evening tries read; a code
  in view that cannot be read, and a dense one too damaged to read, both noticed as such; where a code is, also in a
  soft picture; a picture whose size does not fit its bytes; and made codes read back, all of an animation the same
  size.
* **19 for the phone app's library** (`seedlock-ffi`): a backup opens with its password only; new seeds and their
  backups, reopened; dice rolls in dice-only mode give the seed that SeedSigner makes from them (worked out separately
  in Python); typed words checked and compared; backup names follow the program's rules; a backup's QR code carries
  its name and still reads as the backup, the way earlier readers read it; the wallet for Sparrow equals BIP84's test
  vector (xpub and first address); its QR code for Sparrow, a UR `crypto-output` named after the backup, for every
  address type on mainnet and a test network, its fields rebuilding the test vector's xpub; a payment reviewed and
  signed, its signed copy shown as an animated QR code and collected back by the scanner from camera-like pictures,
  byte for byte; an animated code collected from the middle of its loop, with the corners for the outline; a
  backup's code read while other codes are passed over and a damaged one is never taken, and one with its name read
  with the name; a picture of the wrong size refused. Multisig: a wallet read from Sparrow's exported file and kept as
  the command-line program's `.multisig` file, byte for byte, and read back; wrong ones refused, saying why; a seed's
  key for a multisig wallet laid out for Sparrow (its UR `crypto-account` and its fields) for each kind and network,
  and taken from a wallet it is a cosigner of, but not from one that only names its fingerprint; a 2-of-3 payment
  reviewed and signed by two cosigners in turn, with the program's texts, and refused, with the phone's wording, while
  the wallet is not on the phone, and, after each signature, how many it has and how many more it needs; the
  fingerprints of the wallets a transaction is for; a legacy wallet's payment, whose network comes from the wallet;
  and Sparrow's animated `crypto-output` code, the descriptor as text and a Taproot wallet's code, read through the
  camera scanner.

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
password; the menu and its wording; the backups folder and the name prompt (including a taken name); choosing a backup
by number, by name and by dropping the file (as Linux terminals, Apple's Terminal and `file://` addresses paste it);
that SeedLock works only offline (online, a box naming the connection and nothing else offered, Enter checking again
and opening the menu once disconnected, q quitting with exit code 3; offline said once; an unreadable routing table
and a command refused as well; the virtual machine's own explanation), and that it stops at once when the computer
goes online while it is open: the screen and its scrollback cleared while a backup's words were shown, and the
terminal put back while it read hidden typing or single keys; the live dice counter (it moves with every key; a
non-die, Backspace, an arrow key, changing the last roll); Ctrl+C at a hidden prompt, after which the terminal shows
what you type again; that no `--force` text appears anywhere; that typed secrets are never echoed; a window that is
too small; a new backup shown as a QR code (menu 2), saved as a picture next to it first, where it is said before the
code is drawn, and the picture read back as the backup and its name; the QR code drawn in the terminal (captured from
an emulated screen, decoded with OpenCV and reassembled by Foundation Devices' reference UR library into exactly the
signed PSBT, using the fixed, theme-proof colour codes and wrapped in synchronized output); and Apple's Terminal (with
`TERM_PROGRAM` set the way that app sets it): the boxed note, then every frame of the animated code, the static code
and a backup's code in the full style, read back from the raw output (no block characters; every square two spaces of
one colour), decoded, and reassembled into exactly the signed PSBT or the backup text and its name line; with it
unset, the compact style and no note.

### The QR and camera flows

`tools/qr_flow_test.py` tests them end to end with independent tools:

* menu 8's PNG is read back by OpenCV and equals the backup text followed by its name line; where it is saved is said
  before the code is drawn, and again in the offer to copy it;
* menu 6's explanations, file list, wrong-path handling and the network found by itself are driven in a
  pseudo-terminal;
* a backup is read from its QR code, with its name line, through the **real ffmpeg** (a soft, noisy, compressed video
  that shows a transaction's code first, which is passed over): it opens (menu 4) and signs (menu 6, the signature
  verified by `embit`); menu 8's copy goes to a dropped folder;
* menu 5's fingerprint, `m/84'/...` derivation, xpubs of all four address types and first address equal `embit`'s,
  on mainnet (found from the wallet's own transaction) and on a test network (after the one question); its descriptor
  checksum agrees with BIP380's reference code; the saved QR picture, a UR crypto-output, is read by `urtypes` (the
  library SeedSigner uses) as `embit`'s own account key with its origin, its keystore named after the backup;
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
  code reads back as exactly the backup text and its name line (where its picture is saved said before the window
  opens), typing `t` switches to the terminal, Enter closes the window, without a screen or without ffplay
  SeedLock says why and draws the code in the terminal, and when the computer goes online while the window is open,
  SeedLock stops and the window closes with it at once.

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
  file, found in the backups folder, even when the code carries another name; for a backup kept elsewhere it takes
  the name the code carries (here with accents: the files get the whole name, the keystore its first 16
  characters), and only for a code without a name (made before 0.2.0) it asks.
* Codes in Coinkite's BBQr format, made by Coinkite's own reference code: a wallet registered from its descriptor as a
  compressed BBQr series, and its transaction scanned as a compressed BBQr series and signed; embit verifies.
* A whole wallet in one static QR code, as Sparrow shows it at medium density (UR and BBQr), drawn the way Sparrow
  draws it and photographed on a screen: registered through the real ffmpeg (the BBQr photo is one the reader could
  not read before its light-evening tries). While a damaged copy is in view, SeedLock says it sees a code it cannot
  read, and what helps.

### The phone app, checked on a computer

Besides what was tried on a real phone (see [Confirmed on real computers](#confirmed-on-real-computers)), this was
checked on a computer:

* `android/jvm-check/run.sh` calls the library the way the app does, through the Kotlin code that `uniffi-bindgen`
  writes and JNA, but in Java's virtual machine on a computer: the BIP39 test seed and its wallet for Sparrow; a wrong
  word and a wrong password refused, each with the right kind of error; a backup made and reopened; its QR code with
  its name, read by the scanner; a transaction made by `embit` (an independent Python library) read from its file,
  reviewed and signed; the signed transaction's animated QR code read back through the scanner; `embit` verifies the
  signature over its own signature hash (and, as a control, no longer once an amount is changed); and `urtypes` (the
  library SeedSigner uses) reads the wallet's QR code for Sparrow as `embit`'s own account key, with its name.
  Multisig: a 2-of-3 wallet that `embit` describes the way Sparrow exports it, read and kept as a `.multisig` file,
  its first receive address equal to `embit`'s, its descriptor's QR code read by the scanner; a cosigner's key, whose
  QR code is byte for byte the `crypto-account` that `urtypes` writes for that key; the wallet's payment, which names
  its three cosigners' fingerprints, refused while the wallet is not on the phone, then reviewed and signed by two of
  its cosigners in turn, each signature followed by how many it has; and `embit` verifies both signatures over its own
  signature hash.
* The wallet's QR code for Sparrow, for all four address types on mainnet and a test network, read once with Sparrow's
  own UR library (hummingbird, built from its source) the way Sparrow's scanner reads it: each gives the right address
  type, the key's origin and the backup's name as the keystore's label.
* The app's Kotlin code compiles with the Kotlin compiler against Android's own classes (those of Android 15, which
  the app is built with), the Kotlin code from `uniffi-bindgen` and JNA.
* The library type-checks for all three kinds of phone processor (64-bit ARM, 32-bit ARM and x86-64), with Rust's
  standard library built for Android.
* `android/build-rust.sh` and `build-rust.ps1` were run with stand-ins for rustup, cargo and cargo-ndk: their steps,
  the user name kept out of the library, and a library that still holds it refused. `build-rust.sh` was run again
  with the real cargo (only rustup and cargo-ndk stand-ins): it builds the helper and writes the license notices.
* The check of a release APK (`seedlock-tools check-apk`), on APKs made with Android's own tools: the manifest and
  the resources linked by aapt2, the code turned into DEX by dx (the app's, Kotlin's standard library, JNA's AAR and
  the annotations: 1390 classes), JNA's real native libraries, zipalign, and apksigner signing with APK Signature
  Schemes v2 and v3. A right one passes, with the certificate fingerprint that apksigner and keytool show; with the
  fingerprint written in, it is copied into `dist` with its SHA-256, a second run finds it there, another file of
  that name is never replaced, and an APK signed with another key (also an EC key) is refused. A build that can be
  debugged, the test build's id, Android Studio's test key, only an old v1 signature, the INTERNET permission, and no
  notices with the home folder's path in a library are each refused, saying why.

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
* **Android** (the phone app's preview, on a Xiaomi Mi 8 with Android 10): built with Android Studio as
  `android/README.md` says, installed, and started from its icon; the online warning and the offline message; a
  backup added by scanning its QR code; the wallet imported into Sparrow from the phone's QR code; a copy of a backup
  saved as a file; a transaction scanned from Sparrow with the phone's camera, signed, and the signed one scanned back
  into Sparrow; backups that already existed on the computer added to the phone and used to sign, with everything
  working in both directions; the additions made after the first test (the backup's name carried by its QR code,
  the QR code saved as a picture, the keystore named after the backup in Sparrow, the new password copied for a
  password manager, the place to save files chosen); multisig (2 of 3, on testnet): the phone's cosigner key
  scanned by Sparrow, which named the keystore after the backup, the multisig wallet added to the phone from
  Sparrow's QR code, and its payment signed on the phone, then finalized and broadcast by Sparrow; and, after the
  fix for Android 10's file picker: a new build installed over the old one with its backups and multisig wallets
  kept, a payment signed from the `.psbt` file that Sparrow saved (a multisig wallet's too), the "Saved" message
  saying where a file went, each backup's fingerprint beside its name and the multisig wallet's screen naming its
  cosigners among the backups, the review and the signed payment saying how many signatures it has and needs, and a
  multisig wallet registered on the computer whose payments the backups on the phone sign.
* **Version 0.2.0**, on the users' Linux, macOS and Windows computers: the version in the menu's title; menu 8's QR
  code, which carries the backup's name, scanned by the phone, which suggested that name; the phone's QR code of a
  backup that is not on the computer, scanned in menu 5, whose name SeedLock then used; menu 5's QR code imported
  into Sparrow (**File > Import Wallet > Output Descriptor > Scan...**), which showed the same first addresses and
  named the keystore after the backup; and Sparrow's animated code scanned in menu 6 and the camera test (menu 9),
  with the QR reader now shared with the phone app.
* **Version 0.2.1:** on the computer, the QR pictures saved before the QR code is shown, and said where (menus 1, 2, 5
  and 8), menu 1 now saving the backup's picture too; on the phone, the prompt for a backup's password waiting while
  the password was copied from a password manager on the phone, the clipboard emptied afterwards, and everything
  closed after more than 5 minutes away. SeedLock's Android key, made with Android Studio's wizard ([The Android
  signing key](#the-android-signing-key)), and the release APK that the wizard built and signed with it (APK Signature
  Scheme v2), which `check-apk` passed: the app and its version, a release build, its permissions, its native
  libraries, its code (all from the libraries its notices name), its license notices, no home folder's path, and the
  certificate's fingerprint. On the Xiaomi Mi 8, that ready-made app installed in place of a SeedLock built before
  0.2.1 (the backups and multisig wallets saved as files, the old app uninstalled, then added again), a testnet
  payment signed with it, the screen **About SeedLock and its licenses**, and a test build installed beside it as
  "SeedLock test".
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
4. The keystore named after a backup read from its QR code in menu 10, 1 (in menu 5 it was confirmed); the macOS
   online check (`route`) and virtual machine check (`sysctl`).
5. Nested and legacy multisig, and one large static UR code: checked here only (with urtypes and embit, and with
   photos of Sparrow's exact codes on a screen).
6. The phone app (a preview), on a real phone: adding a backup or a multisig wallet from its file (the same file
   picker as a transaction's file, which works), nested and legacy multisig wallets, and other phones and Android
   versions than the Xiaomi Mi 8 with Android 10; see
   [The phone app, checked on a computer](#the-phone-app-checked-on-a-computer).
7. That SeedLock works only offline (new in 0.3.0), on real computers and phones: the message, Enter checking again,
   and the stop when a connection appears while it is open, on Linux, macOS and Windows, and on the phone.

Menu 9 and `seedlock camera` let anyone check their own camera setup.

### Re-running the tests

```sh
cargo test --workspace --locked
pip install pexpect opencv-python-headless numpy cbor2 embit mnemonic pyte segno urtypes
git clone https://github.com/Foundation-Devices/foundation-ur-py
git clone https://github.com/coinkite/BBQr
pip install pyqrcode            # for BBQr's reference code; where it fails (recent Ubuntu): pip install --use-pep517 pyqrcode
npm install jsdom               # in the folder you run the tests from, or set JSDOM_DIR to its node_modules
cargo build --release --locked -p seedlock-cli --features test-hooks    # a build for the tests (see The online check)
python3 tools/interactive_test.py target/release/seedlock --ur-lib foundation-ur-py
python3 tools/qr_flow_test.py target/release/seedlock --ur-lib foundation-ur-py
python3 tools/multisig_test.py target/release/seedlock --ur-lib foundation-ur-py --bbqr-lib BBQr/python
```

They also need ffmpeg (with ffplay), Xvfb (a virtual X screen) and Node.js. The independent cross-checks (with the
same build; on a computer that is offline, any build of SeedLock works for them):

```sh
pip install bip-utils embit mnemonic argon2-cffi pynacl
python3 tools/crosscheck.py --bin target/release/seedlock
python3 tools/crosscheck_psbt.py target/release/seedlock
python3 tools/crosscheck_backup.py target/release/seedlock
python3 tools/recover.py --selftest
```

The phone app's library, called from Kotlin as the app calls it (Linux and macOS; it needs a JDK, the Kotlin compiler,
JNA's jar, and `pip install embit mnemonic urtypes`, as the script's first lines say):

```sh
KOTLINC=/path/to/kotlinc/bin/kotlinc JNA_JAR=/path/to/jna.jar sh android/jvm-check/run.sh
```

## Technical notes

### The online check

`netcheck.rs` looks for a way out to other networks, a default route, in the computer's own routing table, without
sending anything: on Linux in `/proc/net/route` and `/proc/net/ipv6_route` (an IPv4 or IPv6 default route that is up,
except the "unreachable" one Linux keeps on the loopback interface), on macOS in the output of
`/sbin/route -n get default` (and `-inet6`), on Windows in the output of `route.exe print`, read in a way that does
not depend on the system's language. The interfaces it goes through are named in the message.

SeedLock works only when this says offline (`offline.rs`). It checks before anything else: online, or where it
cannot tell, the menu shows only what it found (Enter checks again, `q` quits) and a command stops; either way the
exit code is 3, and only `--help` and `--version` answer (clap handles them first). Then a thread checks every
second: online (or unknown three times in a row), it puts the terminal back as SeedLock found it (the settings saved
at the start, in case hidden typing or the dice counter's single keys were being read), clears the screen and the
scrollback, says why and ends the process, with which the QR window and the camera's programs end too. There is no
option to work online.

A virtual machine is recognised from the firmware description it shows: on Linux `/sys/class/dmi/id/sys_vendor` and
`product_name`, on Windows `reg query HKLM\HARDWARE\DESCRIPTION\System\BIOS`, on macOS
`sysctl -n kern.hv_vmm_present`.

The tests stand in their own routing tables and firmware descriptions, in the format of Linux's `/proc/net` on every
system, with `SEEDLOCK_TEST_NET_DIR`. Only builds for testing read it: debug builds (`cargo test`) and builds with the
feature `test-hooks` (`cargo build --release --locked -p seedlock-cli --features test-hooks`, which the test scripts
need and check for). A released program cannot be made to believe that the computer is offline: that code is not in
it, and `tools/build-release.sh` and `build-release.ps1` refuse a program that contains it.

### Camera scanning

SeedLock reads the camera through ffmpeg, started as a separate program, so that no native camera libraries (hard to
build and to audit) are part of SeedLock: `v4l2` on Linux, `avfoundation` on macOS, `dshow` on Windows. It asks for
1280 x 720 pictures first and falls back to 640 x 480. Each picture is read by `seedlock-qr`, the reader the phone app
uses too: with `rqrr` as it is, then enlarged, sharpened and reduced, and then with its light evened out (local mean
and contrast), also after a slight blur that washes out a screen's pixel pattern, within a time budget per picture.
Animated UR codes (`ur:crypto-psbt`, `ur:crypto-output`) are collected in any order (extra parts make up for missed
ones); BBQr codes (Coinkite's format: hexadecimal, base32, or deflate-compressed, which is unpacked to 16 MB at most so
that it cannot fill the memory) are collected the same way. The preview window is an `ffplay` that receives the
pictures through a pipe, with an outline around codes that were read and a progress bar drawn in.

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
menu 5's wallet a `ur:crypto-output`, which Sparrow's wallet import reads and which also names the keystore (before
0.2.0 it was the plain output descriptor text, which Sparrow reads too). A backup's code is its text form with the
backup's name on a second line ([FORMAT.md, section 6](FORMAT.md#6-text-form)).

### Backup format

Documented byte for byte in [`FORMAT.md`](FORMAT.md): Argon2id (256 MiB by default) and XChaCha20-Poly1305, the
header authenticated, a fixed size of 100 bytes, and a `SEEDLOCK1:` + Base32 text form with a checksum that fits in
one QR code. The key-derivation settings (`light` 64 MiB, `standard` 256 MiB, `strong` 1 GiB) are stored inside each
backup. [`tools/recover.py`](../tools/recover.py) is an independent decryptor of about 70 lines, so backups outlive
this program.

## Code layout

```
crates/seedlock-core      the library: mnemonic, entropy, backup format, wallet derivation, multisig, PSBT, QR frames (no I/O)
crates/seedlock-cli       the `seedlock` program: prompts, menu, files, QR pictures and pages, camera scanning, and the
                          online check that keeps it from working on a computer that is not offline (offline.rs)
crates/seedlock-qr        reading QR codes in camera pictures, and making QR codes (no I/O; used by both programs)
crates/seedlock-ffi       the phone app's library: what the app calls, made callable from Kotlin by UniFFI
crates/uniffi-bindgen     writes the Kotlin code that calls seedlock-ffi (run by android/build-rust.sh)
crates/seedlock-tools     the build helper: the phone app's license notices, and the check of a release APK (never in SeedLock)
android/                  the Android app (preview): its README, build-rust.sh and .ps1, the app, jvm-check/
docs/GUIDE.md             the user guide
docs/DEVELOPMENT.md       this page
docs/FORMAT.md            the backup format specification, with a test vector
docs/release-key.asc      the public release key that signs the checksum lists of the releases
docs/logo/                the logo (SVG and PNG files) and make_logo.py, which draws them: all rights reserved, not MIT
.cargo/config.toml        build settings: the C runtime inside the Windows program
install.sh                builds the program and installs it, so that `seedlock` starts it from any folder
LICENSE                   the MIT license (for everything except the logo)
tools/                    recover.py, crosscheck*.py, interactive_test.py, qr_flow_test.py, multisig_test.py, audit-offline.sh,
                          build-release.sh and build-release.ps1 (the ready-made programs of a release)
dist/                     made by build-release.sh and .ps1: the ready-made programs (never committed)
seedlocker backups/       made where you run seedlock: your backups and their QR pictures (never committed)
seedlocker transactions/  made where you run seedlock: signed files of scanned transactions, and QR pages (never committed)
```

## Limitations and roadmap

Today: the English word list only; multisig as native, nested or legacy SegWit (no Taproot multisig); the unsigned
PSBT comes in as a file or by camera (animated `ur:crypto-psbt`, BBQr, or a plain PSBT in one QR code), and the
signed one leaves as a file or as an animated QR code; a command-line program, and an Android app in preview.

**Phone.** The Android app is a preview (see [The phone app](#the-phone-app-preview)): backups, the wallet for
Sparrow, signing with the camera, and multisig (cosigner keys, multisig wallets, signing as a cosigner). From 0.2.1 on
it is in the releases, signed with SeedLock's own Android key, with its checksum in the signed list. Next steps: more
tests on real phones; an iPhone app from the same library (UniFFI writes Swift as well). The phone and the
command-line program write the same backups and the same QR codes (the backup's name in its QR code, the wallet for
Sparrow as a QR code that names the keystore), so either one opens what the other made (use `--kdf light` for very old
phones).

**Reproducible builds.** Today a program that a user builds is not byte for byte the same file as a release's
ready-made program (file paths and the compiler versions are recorded in it). Building the release programs in a fixed
environment (the same Rust and C compiler versions and the same paths, for example in a container) would let anyone
rebuild a release and compare the SHA-256 with `SHA256SUMS.txt`, without having to trust the release key.
