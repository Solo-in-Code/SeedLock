# SeedLock for Android (preview)

The same SeedLock, as an app for an Android phone that stays offline: a spare phone in airplane mode
takes the place of the offline computer. It opens the backups made on the computer (and the computer
opens the phone's), lays the wallet out for Sparrow, reads transactions from Sparrow's QR code with the
camera, shows you every address and amount, signs, and gives the signed transaction back as an animated
QR code. It also creates new seeds and their backups, and signs as one of the cosigners of a multisig
wallet (for example 2 of 3), as the computer's menu 10 does.

**This is a preview.** Try it with a test wallet (testnet) or a small amount first. From version 0.2.1
on, every release has the app ready-made, signed with SeedLock's own key; you can also build it yourself.

What makes it safe to use on a phone:

* **It cannot use the internet, and it works only offline.** The app does not have Android's INTERNET
  permission, so Android itself refuses it any network connection. And it works only while the phone is in
  airplane mode with no network connected: otherwise its first screen shows only a message, and if the
  phone goes online while SeedLock is open, it closes everything at once.
* **The seed stays in SeedLock's Rust code** (`crates/seedlock-core`, the same as the computer's), in
  memory that is wiped when it is no longer needed. Leaving the app (the home button, another app, the
  screen switching off) closes the open backup.
* **No screenshots** and no screen recording of SeedLock, no picture of it in the list of recent apps,
  no keyboard learning or autofill in its fields, no copy of its files in Google's cloud backup or on a
  new phone. A seed's words and a new password cannot be copied, so they never reach the clipboard.
* **The backups on the phone are the same encrypted files** as on the computer, in the app's own storage,
  which other apps cannot read. Once a backup was opened on the phone, SeedLock also keeps its wallet's
  fingerprint there, to show it beside the backup's name; nothing about a BIP39 passphrase is ever kept.

## Installing the ready-made app

On the online computer:

1. **Download it.** On SeedLock's GitHub page, click **Releases** on the right. Under the newest release,
   in its **Assets**, download `seedlock-0.3.0-android.apk` (the version may be newer) and
   `SHA256SUMS.txt`.
2. **Check it,** as for the computer programs (the main README, Part 1, Way A, step 2). Open a terminal
   in the folder it was downloaded to and type the line for your computer:

   | Computer | Command |
   |---|---|
   | Windows | `(Get-FileHash seedlock-0.3.0-android.apk).Hash` |
   | Mac | `shasum -a 256 seedlock-0.3.0-android.apk` |
   | Linux | `sha256sum seedlock-0.3.0-android.apk` |

   The long number it shows must be exactly the one next to the file's name in `SHA256SUMS.txt`. If it is
   not, delete the file. To check that list's signature too, see the guide:
   [Checking a downloaded program](../docs/GUIDE.md#checking-a-downloaded-program).
3. **Copy it to the phone** (USB cable, SD card or USB stick) and open it with the phone's **Files** app.
   Android asks to allow installing apps from that app: allow it, install, and then take the permission
   back (**Settings > Apps > Files > Install unknown apps**).

Then put the phone in **airplane mode** and keep Wi-Fi, mobile data and Bluetooth off: SeedLock works only
then. Until the phone is offline, its first screen shows only a message (it checks every second).

**Its signature.** The app is signed with SeedLock's own Android key. Android checks that by itself: it
installs an update only when the update is signed with the same key, and the backups on the phone stay.
The SHA-256 fingerprint of that key's certificate is:

```
02:27:2A:CC:06:84:2D:36:3A:19:A6:CF:48:3D:D9:28:5F:CB:CA:C5:38:AB:98:24:51:14:35:2A:B9:68:63:44
```

(Android's `apksigner verify --print-certs` shows it too, in small letters and without the colons.)

**If you built SeedLock yourself before version 0.2.1,** that app is also called SeedLock, but it is
signed with your computer's test key, so Android does not let the ready-made app replace it. Move to the
ready-made app this way:

1. In the old app, open each backup and choose **Copy it > Save a copy as a file**, and on each multisig
   wallet's screen **Save a copy as a file** (or make sure you still have those files on the computer).
2. Uninstall the old SeedLock (**Settings > Apps > SeedLock > Uninstall**). This deletes the backups on
   the phone: do it only once you have their copies.
3. Install the ready-made app as above, and add the backups and the multisig wallets again (**Add a
   backup > Open its file**, **Add a multisig wallet**).

## Building it yourself

You build the app on one of your computers (Linux, Mac or Windows) and copy it to the phone once. The
building computer needs the internet; the phone never does.

What you build is a **test build**: it installs as **SeedLock test** (`org.seedlock.app.debug`), an app
of its own beside the ready-made SeedLock, with backups of its own. Trying a build never touches the app
you use.

### What you need, once

1. **Rust**, installed with rustup, as for SeedLock itself (the main README, Part 1, Way B).
2. **cargo-ndk**, a small tool that builds Rust for Android. In a terminal:

   ```sh
   cargo install cargo-ndk
   ```

3. **Android Studio**, free, from [developer.android.com/studio](https://developer.android.com/studio).
   Install it and start it once; it downloads the Android SDK (several GB).
4. **The Android NDK**, inside Android Studio: on its welcome screen click **More Actions > SDK Manager**
   (or, with a project open, **Tools > SDK Manager**), open the tab **SDK Tools**, tick **NDK (Side by
   side)** and click **OK**.

### Each time

1. **Build the Rust part.** In a terminal, in SeedLock's folder (the one with `Cargo.toml`):

   | System | Command |
   |---|---|
   | Linux, macOS | `sh android/build-rust.sh` |
   | Windows (PowerShell or Command Prompt) | `powershell -ExecutionPolicy Bypass -File android\build-rust.ps1` |

   It builds `libseedlock_ffi.so` for 64-bit phones, older 32-bit phones and the emulator, writes the
   Kotlin code that calls it, and writes the license notices that the app shows (from exactly the
   libraries it built). It ends with `Ready:` and the files it made. The first time takes several
   minutes.

2. **Open the folder `android` in Android Studio** (**File > Open**, then choose the `android` folder
   inside SeedLock's folder, not SeedLock's folder itself). The first time, Android Studio downloads
   Gradle and Android's build tools ("Gradle sync"), which takes a few minutes. If it says that an SDK
   platform is missing, click the link it shows to install it.

3. **Build the app:** **Build > Generate Bundle(s) / APK(s) > Generate APK(s)** (in older versions of
   Android Studio: **Build > Build Bundle(s) / APK(s) > Build APK(s)**). When it is done, a message links
   to the file: the app is `app-debug.apk`, in `android/app/build/outputs/apk/debug`. It is the only
   file to install (the project builds no separate test app). The ready-made app of a release is built
   differently, signed with SeedLock's own key: `docs/DEVELOPMENT.md`, Making a release.

   A warning that the INTERNET permission "was tagged ... to remove other declarations but no other
   declaration present" is expected: it says that no library asked for the internet, so there was nothing
   to take out.

### Try it in the emulator first

In Android Studio: **Tools > Device Manager**, add a phone (any recent one), then click the green
**Run** button at the top. The emulator's camera can be your computer's webcam: in the Device Manager,
edit the virtual phone and, in its settings (**Show Advanced Settings** or **Additional settings**,
depending on the version), set the back camera to your webcam (**Webcam0**). As on a phone, SeedLock
works only once the emulator is in airplane mode (pull down its quick settings).

### Install it on the phone

Either:

* **With a USB cable, from Android Studio.** On the phone: **Settings > About phone**, tap **Build
  number** seven times (this turns on the developer options), then **Settings > System > Developer
  options > USB debugging** on. Connect the phone, allow the computer on the phone's screen, choose the
  phone at the top of Android Studio and click **Run**: it installs SeedLock test and starts it.
  Afterwards, turn **USB debugging** off again.
* **Or with adb**, with USB debugging on as above. In a terminal, in SeedLock's folder (`adb` is in the
  folder `platform-tools` of the Android SDK: `~/Android/Sdk` on Linux, `~/Library/Android/sdk` on a Mac,
  `%LOCALAPPDATA%\Android\Sdk` on Windows):

  ```sh
  adb install -r android/app/build/outputs/apk/debug/app-debug.apk
  adb shell am start -n org.seedlock.app.debug/org.seedlock.app.MainActivity
  ```

* **Or as a file.** Copy `app-debug.apk` to the phone (USB cable, SD card or USB stick) and open it with
  the phone's **Files** app. Android asks to allow installing apps from that app: allow it, install, and
  then take the permission back (**Settings > Apps > Files > Install unknown apps**).

**On Xiaomi phones** (MIUI, HyperOS) the developer options are turned on by tapping **MIUI version** (or
**OS version**) seven times in **About phone**, and they are in **Settings > Additional settings >
Developer options**. There, Android Studio and adb can install apps only when **Install via USB** is on
too (Xiaomi asks you to sign in to a Mi account for it), and the phone asks you to confirm each
installation on its screen. Installing the file with the phone's file manager works without it.

SeedLock test then has its icon among the phone's apps. With adb, `adb shell pm list packages seedlock`
must show `package:org.seedlock.app.debug` (and `package:org.seedlock.app` if the ready-made SeedLock is
installed too). An earlier build may have left a test app without an icon,
`package:org.seedlock.app.test`: remove it with `adb uninstall org.seedlock.app.test`.

**A newer build** installs over the old one, in any of these ways, and the backups on the phone stay, as
long as both builds come from the same computer (Android Studio signs them with that computer's own
test key). A build from another computer is refused as an update: then first save a copy of each backup
(**Save a copy as a file**), uninstall SeedLock test, install the new build and add the backups again.
Builds from before version 0.2.1 were called SeedLock, not SeedLock test: a newer build installs beside
such an old one, so move its backups over (save their copies, add them) and then uninstall the old one.

Then put the phone in **airplane mode** and keep Wi-Fi, mobile data and Bluetooth off: SeedLock works only
then. Until the phone is offline, its first screen shows only a message (it checks every second).

## What to try

1. **Add a backup** made on the computer: in SeedLock on the computer, menu 8 shows it as a QR code; on
   the phone, **Add a backup > Scan its QR code**. Or copy its `.seedlock` file to the phone and choose
   **Open its file**: then its file name is offered as the backup's name. (The QR codes of SeedLock 0.2.0
   and newer carry the backup's name, on the computer and on the phone, see 8; older ones do not.)
2. **Check the backup**: its password opens it, and **Compare** says MATCH for the words on paper. With
   the password in a password manager on this phone: copy it there and come back to SeedLock within 5
   minutes to paste it (the prompt waits that long); when the prompt closes, SeedLock empties the
   clipboard.
3. **Wallet for Sparrow**: in Sparrow on the online computer, **File > Import Wallet > Output Descriptor >
   Scan...** and hold the phone's screen to the camera. Sparrow must show the same fingerprint and the
   same first addresses as the phone. Sparrow names the wallet's keystore after the backup (its first
   16 characters: Sparrow allows no more); when Sparrow asks for the wallet's name, type the backup's
   name too.
4. **Sign a transaction** (best on testnet first): in Sparrow, make a payment, click **Show QR** and scan
   it with the phone. Check every address and amount on the phone, sign, and in Sparrow click **Scan QR**
   and hold the phone's screen to the camera. Sparrow then shows the transaction as signed. Or, in
   Sparrow, click **Save Transaction**, copy the `.psbt` file to the phone (for example into its
   Download folder) and choose **Open the .psbt file that Sparrow saved**; then **Save it as a file
   instead** gives the signed one back as a file.
5. **Create a new seed** on the phone, then open its backup on the computer (copy the file, or show it as
   a QR code): SeedLock on the computer must open it with the same password and show the same fingerprint.
   With a password manager on this phone, **Copy it** copies the generated password: paste it there,
   save it, copy it back and come back to SeedLock within 5 minutes (it stays open meanwhile) to paste it
   in. Coming back, SeedLock empties the clipboard if its own copy of the password is still on it.
6. **Leave the app** in the middle of something (home button): coming back, SeedLock is on its first
   screen again. A screenshot of SeedLock shows nothing.
7. **Its permissions:** in **Settings > Apps > SeedLock** (or **SeedLock test**) **> Permissions**, the
   camera is the only one. Where the menu (**⋮**) there offers **All permissions**, "have full network
   access" is not in the list.
8. **Copy a backup** (open it in **Backups on this phone**, then **Copy it**): **Show it as a QR code**
   and scan it with SeedLock on another phone, where the backup's name comes along; **Save its QR code as
   a picture** (to print or keep) and **Save a copy as a file**. Android asks where to save: its menu
   (☰) offers the phone's folders, an SD card, a USB stick, and a cloud folder if the phone has one. A
   cloud folder uploads only when the phone is online, so for a phone that stays offline, an SD card or a
   USB stick is the way to your computer (and from there to the cloud). SeedLock then says where the file
   is, and the next time Android starts in the same folder.
9. **Multisig** (on testnet first), with the phone's backup as one of the cosigners, for example of a
   2-of-3 wallet whose two other keystores are new software wallets in Sparrow, or other seeds:
   1. **The phone's key.** Open the backup on the phone, **Key for a multisig wallet**: choose the network
      and the kind (Native Segwit, the usual one). In Sparrow: **File > New Wallet**, *Policy Type* "Multi
      Signature", 2 of 3, the same *Script Type*. For the phone's keystore click **Watch Only Wallet** (in
      Sparrow before 2.5: **xPub / Watch Only Wallet**), then the **camera button** next to the *xpub*
      field, and hold the phone's screen toward the computer's camera: Sparrow fills in the three fields
      and names the keystore after the backup. Fill in the other keystores and click **Apply**.
   2. **The wallet on the phone.** On the phone's first screen: **Add a multisig wallet > Scan Sparrow's
      QR code**; in Sparrow, the wallet's **Settings** tab, the QR code button next to *Descriptor* (under
      *Script Policy*). Or open the file that Sparrow exports (**File > Export Wallet > Output
      Descriptor**). The phone shows the cosigners and the wallet's first receive address: Sparrow must
      show the same one (its **Addresses** tab). Add it under a name.
   3. **Sign.** Make a payment from the multisig wallet in Sparrow and sign it on the phone, as in 4: with
      the backup's **Sign a transaction**, or on the multisig wallet's screen, with **Sign a transaction
      with "..."**, one button for each backup on the phone that is one of its cosigners. A blue box at
      the top of the review says which cosigner the backup is and how many signatures the payment needs;
      the inputs and the change say that they are the multisig wallet's ("verified from all its cosigners'
      keys"). After signing, the blue box says how many signatures it has now, and how many more
      cosigners must sign. Scan the signed payment into Sparrow, let a second cosigner sign (in Sparrow,
      a software keystore signs with its **Sign** button), then Sparrow finalizes and broadcasts it.
   4. The multisig wallet on the phone, **Save a copy as a file**: the same `.multisig` file as SeedLock on
      the computer keeps (in its `multisig wallets` folder, inside `seedlocker backups`), and Sparrow opens
      it too (**File > Import Wallet > Output Descriptor**).
10. **Fingerprints.** Once a backup was opened on the phone (**Check the backup**, for example), its
    wallet's fingerprint shows beside its name, on the first screen and on its own screen. A backup added
    from a file or a QR code shows it only once it was opened here, even one first made on this phone:
    the backup is encrypted, and SeedLock writes the fingerprint into neither its file nor its QR code,
    where anyone could read it. A multisig wallet's screen then names the backups on the phone that are
    its cosigners. Start signing a transaction with a backup that is not one of the transaction's
    wallets: before asking for the password, the phone says so, and which backup on the phone is. (With a
    BIP39 passphrase, a seed is another wallet, with another fingerprint: then open the backup anyway.)
11. **About and licenses.** At the bottom of the first screen, **About SeedLock and its licenses** shows
    the version, where the source code is, and the license notices of every library the app is built
    from.
12. **The ready-made app** (from version 0.2.1 on): install it as **Installing the ready-made app** says.
    It installs beside SeedLock test, as SeedLock; add a backup to it and sign with it as in 1 and 4.
13. **Offline only** (from version 0.3.0 on). Open a backup's screen, then turn airplane mode off in the
    quick settings: within a second SeedLock closes everything, and its first screen says that it works
    only offline, and why (airplane mode is off, or a network is connected). Turn airplane mode on again:
    the backups come back by themselves. Do the same while SeedLock asks for a password, or shows a new
    seed's words.

Tell what happened at each step, and what the phone showed when something did not work (its make and
model, and the Android version from **Settings > About phone**).

## For developers

* The app is in `app/src/main/java/org/seedlock/app`: plain Android classes and Kotlin, no AndroidX or
  other library except JNA. Everything about seeds, backups, wallets, transactions and QR codes is in
  Rust: `crates/seedlock-ffi` (the part the app calls), `crates/seedlock-core` and `crates/seedlock-qr`.
* `org.seedlock.ffi` is written by `uniffi-bindgen` from `crates/seedlock-ffi` (build-rust.sh does it);
  it is not kept in git.
* The library is built with the `phone` profile (in `Cargo.toml`): the release settings, except that a
  panic becomes an error that the app shows instead of closing it. `docs/DEVELOPMENT.md` (The phone app)
  says more, and `sh tools/audit-offline.sh` checks the app's permissions, libraries and Kotlin code.
* `jvm-check/run.sh` checks that bridge on a computer, the way the app uses it: Kotlin calls the Rust
  code through JNA, and embit (Python) independently checks the signatures it makes, of a payment and of
  a 2-of-3 multisig payment signed by two of its cosigners.
* `crates/seedlock-tools` is a helper for the computer that builds the app, never part of it:
  `build-rust.sh` runs it to write the license notices the app shows (`app/src/main/assets/notices.txt`,
  not kept in git), and it checks a release APK before it is published (`docs/DEVELOPMENT.md`, Making a
  release).
