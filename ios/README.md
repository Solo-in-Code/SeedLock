# SeedLock for iPhone (in progress)

The same SeedLock as an app for an iPhone that stays offline, as the Android app is for an Android phone.
It is being built step by step. **So far** it makes a new seed and its backup (or the backup of a seed
you already have), keeps your backups on the iPhone, opens a backup with its password to check it or to
show its words, and copies a backup off the iPhone: as a file, as a QR code, or as a picture of it. And
it lays out a backup's wallet for Sparrow: a watch-only wallet, as a QR code for Sparrow to scan, as
three fields to type, or as files, with its first addresses to compare.
Next come, one group at a time: signing with the camera, and multisig.

**Use it only with a test wallet for now.** It is a first preview, and not independently audited.

What makes it safe to use on an iPhone:

* **It works only offline, and it has no way to go online.** iOS lets every app use the internet, with
  no permission an app could go without (as Android has), so SeedLock simply has no code that connects to
  anything; `tools/audit-offline.sh` checks this. And it works only while no network is connected: iOS
  tells no app whether airplane mode is on, so SeedLock looks at the networks, and a VPN that is on
  counts as one. While the iPhone is online, its first screen shows only a message, and if the iPhone
  goes online while SeedLock is open, it closes everything at once.
* **The seed stays in SeedLock's Rust code** (`crates/seedlock-core`, the same as the computer's), in
  memory that is wiped when it is no longer needed. Leaving the app (the home screen, another app, the
  screen locked) closes the open backup, and the open wallet.
* **Only Apple's own keyboard.** SeedLock refuses keyboards from other apps, which could keep what you
  type. In its fields, autocorrection and suggestions are off, and passwords go into password fields,
  which the keyboard never learns from.
* **Hidden when it should be.** In the app switcher SeedLock shows a cover, never its screen, and it
  shows nothing while the screen is recorded or mirrored. iOS lets no app block a screenshot: take none
  while SeedLock shows something secret (if one is taken, SeedLock says what to do).
* **The backups on the iPhone are the same encrypted files** as on the computer, in SeedLock's own
  storage, which other apps cannot read. iOS keeps them encrypted while the iPhone is locked, and they are
  left out of the iPhone's backups (iCloud, or a computer). Once a backup was opened on the iPhone,
  SeedLock also keeps its wallet's fingerprint there, to show it beside the backup's name.

## What you need

* **A Mac with Xcode 16 or newer** (free, from the App Store). Open Xcode once after installing it, and
  let it install what it asks for.
* **Rust, installed with rustup**: the same as for building the computer program yourself (the main
  README, Part 1, Way B).
* **An iPhone with iOS 17 or newer**, and its cable.
* **An Apple ID.** A free one works: the app then opens for 7 days, and after that Xcode puts it on the
  iPhone again (your backups stay).

## Building it and putting it on the iPhone

On the Mac, online:

1. **Build the Rust part.** Open **Terminal**, type `cd ` (with a space after it), drag SeedLock's folder
   from Finder into the Terminal window, and press Return. Then type:

   ```
   sh ios/build-rust.sh
   ```

   The first time it takes several minutes: it downloads Rust's parts for the iPhone and the libraries.
   It ends with **Ready:** and a short list of files.
2. **Open the project in Xcode:** `open ios/SeedLock.xcodeproj`
3. **Add your Apple ID to Xcode:** **Xcode > Settings > Accounts**, the **+** at the bottom, **Apple
   ID**.
4. **Choose your team.** In Xcode's left column, click **SeedLock** (the blue icon at the top), then the
   target **SeedLock** and the tab **Signing & Capabilities**. Under **Team**, choose your name
   **(Personal Team)**. Leave the **Bundle Identifier** as it is: it ends with your team's ID, which
   keeps it different from everyone else's. (Xcode now writes your team into the project file: see
   [For developers](#for-developers).)
5. **Try it in the simulator** (optional). At the top of Xcode, choose an iPhone simulator (for example
   **iPhone 16**), and press **Run** (the ▶ button). The simulator uses the Mac's network, so SeedLock
   says it is online: turn off the Mac's Wi-Fi (and unplug its network cable) while you try it there. If
   SeedLock still says it is online, stop it (the ■ button) and press **Run** again.
6. **Connect the iPhone** with its cable, unlock it, and tap **Trust** on the iPhone.
7. **Turn on Developer Mode on the iPhone:** **Settings > Privacy & Security > Developer Mode** (near the
   bottom; it shows only after the iPhone was connected to Xcode). The iPhone restarts and asks once more.
8. **Run it on the iPhone.** At the top of Xcode, choose your iPhone, and press **Run**.
9. **The first time, trust your Apple ID on the iPhone.** iOS says the developer is not trusted: on the
   iPhone, **Settings > General > VPN & Device Management**, tap your Apple ID, then **Trust**. Press
   **Run** in Xcode again.
10. **Take the iPhone offline:** turn on airplane mode, then check that Wi-Fi and Bluetooth are off too
    (iOS leaves them on in airplane mode if you once turned them on there), and turn off any VPN
    (airplane mode leaves a VPN on, and SeedLock counts it as a network). SeedLock opens as soon as no
    network is connected.

If Xcode stops with an error, its text (with the file's name and the line) is in Xcode's left column,
under the triangle with **!** (the Issue navigator).

**Later:** after 7 days with a free Apple ID, connect the iPhone and press **Run** again. When SeedLock's
Rust code changed (a new version), run `sh ios/build-rust.sh` again before you press **Run**; if Xcode
then shows odd errors, choose **Product > Clean Build Folder** first.

## What to try

Make a test backup on the computer first (menu 1 or 2, with a test seed), and copy its `.seedlock` file
to the iPhone: AirDrop, a USB stick, or a cloud folder in the **Files** app, while the iPhone is still
online. Then take the iPhone offline and open SeedLock:

1. **Add the backup:** **Add a backup** > **Open its file**, choose it, and **Add this backup**. Its name
   comes from the file's name; you can change it. Or paste the backup's text (the line that starts with
   `SEEDLOCK1:`).
2. **Check it:** tap the backup, then **Check the backup**, and type its password. After a few seconds
   SeedLock shows **Backup OK** and the wallet fingerprint, the same as the computer shows. Type the
   words from your paper and tap **Compare**.
3. **Leave and come back:** while **Check the backup** is open, go to the home screen and back: SeedLock
   is on its first screen again, and the backup is closed.
4. **The password from a password manager:** in the password prompt, the key above the keyboard
   (AutoFill) offers your saved passwords. Or copy the password in the password manager app and come
   back within 5 minutes to paste it: SeedLock then empties the clipboard.
5. **A new seed and its backup:** **Create a new seed and its backup**, 12 words (a test seed). Write the
   words down, then type back the three that SeedLock asks for. Give the backup a name, tap **Generate a
   strong password for me**, then **Copy it**, save it in your password manager, come back and let the
   password manager fill it in; then **Encrypt and save**. Or try **Add my own dice rolls**, and **Encrypt
   the words of a seed I already have** with your own password typed twice.
6. **The backup's copies:** on the backup's screen, **Show the words** (after its password), **Show it as
   a QR code** (the Android app's **Scan its QR code** reads it, and the computer's menu 4 with **c**),
   **Save its QR code as a picture** and **Save a copy as a file**, onto **On My iPhone** or a USB stick.
7. **The wallet for Sparrow:** on the backup's screen, **Wallet for Sparrow**, and its password (for a
   usual wallet, leave **Real bitcoin** chosen and the passphrase off). SeedLock shows the watch-only
   wallet: in Sparrow on the computer, **File > Import Wallet**, then **Output Descriptor** and
   **Scan...**, and hold the iPhone's screen toward the computer's camera. Sparrow then shows the same
   first receive addresses as SeedLock. **Script Type** shows the other address types, and **Silent
   payments** the silent payment wallet. **Save the QR code as a picture** and **Save all of it as a text
   file** save what the computer's menu 5 saves.

## How it differs from the Android app

* **Going online.** Android lets an app go without internet at all, and SeedLock does; iOS has no such
  setting, so SeedLock's safety here rests on having no network code, which the audit checks. iOS does
  not tell apps whether airplane mode is on: SeedLock works while no network is connected.
* **Screenshots.** Android blocks them in SeedLock; iOS cannot. SeedLock covers its screen in the app
  switcher and while the screen is recorded or mirrored.
* **Typed passwords.** Swift's text cannot be wiped from memory: SeedLock empties its fields at once,
  and its Rust code wipes its own copy.
* **A new password into a password manager.** Android hands it over by autofill, without the clipboard;
  iOS lets only apps with a web site of their own do that. So SeedLock copies it for this iPhone only
  (not to a Mac or an iPad nearby), keeps the new seed open for 5 minutes while you paste it into the
  password manager, and takes its copy off the clipboard when you are back. Filling the password back in
  works by AutoFill, as when a backup is opened.

## For developers

* `ios/build-rust.sh` builds `libseedlock_ffi.a` for iPhones and for the simulator on this Mac (with the
  `phone` profile of `Cargo.toml`, like the Android app's library, and with your user name kept out of
  it), and puts it into `ios/SeedLockFFI.xcframework` with its C header and module map; it writes the
  Swift code that calls it (`uniffi-bindgen`) and the license notices the app shows into
  `ios/SeedLock/Generated`. These are made on each Mac and never committed.
* `ios/SeedLock.xcodeproj` is in Xcode 16's format: the folder `ios/SeedLock` is synchronized with the
  project, so every file in it is part of the app, with nothing to add by hand. The app is SwiftUI, for
  iPhones with iOS 17 or newer, in Swift 5 mode.
* `sh ios/swift-check/run.sh` checks the Swift side on a computer (a Mac, or Linux with Swift): it calls
  the library through the Swift code from `uniffi-bindgen`, with the app's own storage and helpers.
* **Never upload these from your Mac:** `ios/SeedLock.xcodeproj/project.pbxproj` once you chose your
  team (it then holds your team's ID; the tests fail on it), and the `xcuserdata` folders that Xcode
  makes (they are named after your Mac's user name). Upload the files from SeedLock's source instead.
