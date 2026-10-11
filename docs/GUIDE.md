# SeedLock user guide

This guide explains everything SeedLock does. To install SeedLock, follow the [README](../README.md) first: get the
program on your online computer (Part 1: download it, or build it yourself), copy it to your offline computer
(Part 2) and start it there (Part 3). The README's [First steps](../README.md#first-steps) are the short version of
this guide.

Everything in this guide happens on the **offline computer**, unless it says **in Sparrow**:
[Sparrow Wallet](https://sparrowwallet.com) runs on your everyday, online computer. The Sparrow steps are written for
Sparrow 2.5; where older versions name something differently, this guide says so.

> If you know SeedSigner: SeedLock does the same job on an ordinary computer instead of a small device. It keeps the
> seed offline, signs the payments that Sparrow prepares, and uses the same QR codes.

## Contents

1. [Words used in this guide](#words-used-in-this-guide)
2. [Checking a downloaded program](#checking-a-downloaded-program)
3. [Starting SeedLock](#starting-seedlock)
4. [Your backups and the other files](#your-backups-and-the-other-files)
5. [Create a seed: menu 1](#create-a-seed-menu-1)
6. [More about backups: menus 2, 3, 4, 7 and 8](#more-about-backups-menus-2-3-4-7-and-8)
7. [Set up Sparrow: menu 5](#set-up-sparrow-menu-5)
8. [Sign a payment: menu 6](#sign-a-payment-menu-6)
9. [Multisig wallets: menu 10](#multisig-wallets-menu-10)
10. [QR codes and the camera: menu 9](#qr-codes-and-the-camera-menu-9)
11. [Security advice](#security-advice)
12. [The command line](#the-command-line)
13. [Opening a backup without SeedLock](#opening-a-backup-without-seedlock)

## Words used in this guide

* **Seed**: the 12 or 24 words that are your wallet. Whoever has them can spend your bitcoin.
* **Backup**: a file that holds your seed, encrypted with a password (`<name>.seedlock`). Nobody can open it without
  the password, so it can be kept on a USB stick or in the cloud.
* **Fingerprint**: 8 letters and digits, such as `73c5da0a`, that name a seed. SeedLock and Sparrow both show it, so
  you can check that they mean the same seed. It reveals nothing secret.
* **Watch-only wallet**: Sparrow's copy of your wallet, without the seed. It shows your balance and your addresses
  and prepares payments, but it cannot sign them.
* **PSBT** (partially signed Bitcoin transaction): a payment that Sparrow prepared and that still needs a signature.
  As a file, its name ends in `.psbt`.
* **Passphrase** (BIP39 passphrase, the "25th word"): an optional extra word that turns the same seed into a
  different wallet. Most people do not use one. If you have never set one, you do not use one: answer no when
  SeedLock asks.
* **Mainnet** is real bitcoin. **Testnet** and **signet** are test networks: their coins have no value, so they are
  for trying things out.

## Checking a downloaded program

If you downloaded the ready-made program (README, Part 1, Way A), the README has you compare its fingerprint with
`SHA256SUMS.txt`: that shows that the download is complete and is the file the list names. The signature shows more:
that `SHA256SUMS.txt` itself was made by SeedLock's maintainer, with SeedLock's release key. Someone who managed to
change the files on the release page could change the list too, but could not sign it with that key.

SeedLock's release key has this fingerprint:

```
E435 21D9 44AE 9BAA 9FB2  D11D 08B6 5FB6 5CB1 D806
```

Do this on the online computer, before you copy the program to the offline one:

1. **Install GnuPG,** the free program that checks signatures. Linux: it is usually there already (the command
   `gpg`; otherwise `sudo apt install gnupg`, or `sudo pacman -S gnupg` on Arch). Mac: `brew install gnupg`, with
   [Homebrew](https://brew.sh). Windows: Gpg4win, from [gpg4win.org](https://gpg4win.org).
2. **Download two more files** into the folder that holds the program and `SHA256SUMS.txt`:
   * `SHA256SUMS.txt.asc`, the signature, from the same release's **Assets**;
   * the release key, [`release-key.asc`](release-key.asc): on SeedLock's GitHub page it is in the folder `docs`;
     open it and click the button **Download raw file**.
3. **Open a terminal in that folder** (as in the README, Part 1, Way A, step 2) and type:

   ```sh
   gpg --import release-key.asc
   gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt
   ```

4. **Read what it says.** It must say **Good signature**, and the line `Primary key fingerprint:` must show exactly the
   fingerprint above. It also says `WARNING: This key is not certified with a trusted signature!`. That is normal: it
   only means that you have not certified the key yourself in GnuPG, and comparing the fingerprint is what counts. If
   it says **BAD signature**, or shows another fingerprint, do not use the download.
5. Then compare the program's fingerprint with `SHA256SUMS.txt`, as the README says (Part 1, Way A, step 2).

The same fingerprint is in the README. If you know SeedLock's maintainer from somewhere else, such as a website or a
profile, compare it there too: someone who could change SeedLock's GitHub page could change this text as well.

## Starting SeedLock

### The folder you start it from

SeedLock keeps your backups in a folder named `seedlocker backups`, **inside the folder the terminal is in when you
start SeedLock**. That is why the README starts it from its own folder: the program and your backups stay together,
and SeedLock finds your backups every time.

| System | How to start it |
|---|---|
| Windows | Double-click `seedlock.exe`. Or, in PowerShell in its folder, type `.\seedlock.exe` |
| Linux and macOS | Open a terminal, go to SeedLock's folder (type `cd `, drag the folder onto the window, press Enter) and type `./seedlock` |

If you start it from another folder, for example by typing `SeedLock/seedlock` in your home folder, SeedLock looks
for `seedlocker backups` in your home folder, and your backups are not in its list. On Linux and macOS, do not
double-click the program for the same reason: a double-clicked program may start in your home folder. A backup kept
somewhere else can always be opened by typing or dropping its path (see
[Choosing a backup](#choosing-a-backup)).

### The menu

| Menu | What it does | Details |
|---|---|---|
| 1 | Creates a new seed and its encrypted backup | [Create a seed](#create-a-seed-menu-1) |
| 2 | Makes a backup of a seed you already have: you type its words | [Menu 2](#menu-2-a-seed-you-already-have) |
| 3 | Shows the words in a backup | [Menu 3](#menu-3-show-the-words) |
| 4 | Checks that a backup opens, and optionally that it holds the same words as your paper | [Menu 4](#menu-4-check-a-backup) |
| 5 | Shows what Sparrow needs for a watch-only wallet | [Set up Sparrow](#set-up-sparrow-menu-5) |
| 6 | Checks and signs a payment that Sparrow prepared | [Sign a payment](#sign-a-payment-menu-6) |
| 7 | Generates a strong password | [Menu 7](#menu-7-a-strong-password) |
| 8 | Shows a backup as a QR code, and saves it as a picture | [Menu 8](#menu-8-a-backup-as-a-qr-code) |
| 9 | Tests the camera | [QR codes and the camera](#qr-codes-and-the-camera-menu-9) |
| 10 | Multisig wallets: shares this seed's key, registers a wallet | [Multisig wallets](#multisig-wallets-menu-10) |
| q | Quits | |

Type a number and press Enter. After each task SeedLock comes back to the menu; if something went wrong, it says
`Error:` and why. Ctrl+C stops SeedLock at any moment.

When SeedLock asks a question, the answer in square brackets is what you get by pressing Enter alone: `[24]`, `[1]`.
In a yes-or-no question, `[y/N]` means that Enter answers no, and `[Y/n]` that Enter answers yes.

The menu's title shows SeedLock's version: `=== SeedLock 0.1.2: offline Bitcoin seed tool ===`.
`./seedlock --version` (Windows: `.\seedlock.exe --version`) shows it too.

### Is this computer online?

Before the menu, SeedLock checks whether the computer is connected to a network. It does this without using the
network: it only looks at the computer's own routing table (the list of ways out to other networks, which has a way
out as soon as Wi-Fi or a cable connects the computer). On Linux it reads `/proc/net`; on macOS and Windows it asks
the system's `route` program.

* **Offline:** one line says `This computer is offline: no network connection was found.`
* **Online:** a box says **WARNING: THIS COMPUTER IS ONLINE** and names the connection (`wlan0`, `en0`, on Windows the
  network adapter or its address). Disconnect the computer before you create, open or type a seed: turn off Wi-Fi
  and unplug the network cable. While it stays online, a one-line reminder appears each time the menu comes back.
* **Cannot tell:** SeedLock says so, and asks you to make sure Wi-Fi is off and no network cable is plugged in.

SeedLock itself never uses the network, whatever this check says. The warning is there because, on an online
computer, other programs could see your seed words while they are on the screen.

### In a virtual machine

A virtual machine's network adapter always looks connected, even while the real computer is offline: it is
connected to the virtual machine program, and it is online again as soon as the real computer is. Nothing inside the
virtual machine can see the real computer's connection. SeedLock recognises the usual virtual machine programs
(QEMU/KVM, VirtualBox, VMware, Hyper-V, Parallels and Xen) from the description of the computer they show, and then
says **WARNING: THIS VIRTUAL MACHINE HAS A NETWORK CONNECTION**. Turn off the virtual machine's network adapter (in
the virtual machine program's settings, or in the system inside it): then SeedLock shows it as offline.

## Your backups and the other files

### The backups folder

Every backup is saved in the folder `seedlocker backups`, inside the folder you start SeedLock from. SeedLock creates
the folder the first time it saves something (on Linux and macOS, only your own user may open it) and shows its full
path. Each backup is one file, `<the name you chose>.seedlock`.

**SeedLock never overwrites a file.** No option makes it do so: to replace a backup, you must delete it yourself
first.

**Keep copies of your backups**: copy the `.seedlock` files, or the whole `seedlocker backups` folder, to a USB stick
and/or your cloud storage. A backup is useless without its password, and the password is useless without the file,
so keep them in different places (see [Security advice](#security-advice)).

### Names

When you save a backup, SeedLock asks `Save the encrypted backup as [give-a-name-to-your-backup]:`. Type a name such
as `my wallet` or `savings-2026`:

* Letters, digits, spaces and most punctuation are fine; `/ \ : * ? " < > |` are not, and a name cannot start or
  end with a dot.
* A name that is already taken asks again: it never stops the process, so a new seed is never lost because of a
  name. Names that differ only in upper and lower case count as the same, because Windows, macOS, USB sticks and most
  cloud folders treat `Wallet` and `wallet` as one file.
* A name that does not say what the file is (not `bitcoin seed`) is safer if the file is ever found by someone else.

### Choosing a backup

Wherever SeedLock needs a backup (menus 3, 4, 5, 6, 8 and 10), it lists the backups in the folder:

```
Your backups (folder: /home/me/SeedLock/seedlocker backups):
  1) my wallet
  2) savings-2026
  c) Scan a backup's QR code with the camera (generated in menu 8, or printed or on a phone)
```

* Type the backup's **number**, or its **name**.
* For a backup kept somewhere else, type its **path**, or **drag the file** from your file manager onto the terminal
  window (the terminal then types its path for you) and press Enter. Every terminal's way of writing the path (in
  quotes, with backslashes before spaces, or as a `file://` address) is understood.
* You can also paste the backup's text, the line that starts with `SEEDLOCK1:`.
* With only one backup in the folder, Enter alone chooses it.
* **`c` reads the backup from its QR code** with the camera (menu 8 makes that code: on screen, printed, or as a
  picture on a phone). A window shows what the camera sees; QR codes of other things are passed over. The backup is
  used exactly as if its file had been chosen, and nothing is saved. This choice is offered in menus 3, 4, 5, 6 and
  10. A QR code does not hold the backup's name: when SeedLock needs one (for the files it saves, and for Sparrow's
  keystore label), it uses the name of the same backup in your backups folder, or asks.

### The other files SeedLock makes

| File | Made by | What it is |
|---|---|---|
| `seedlocker backups/<name>.seedlock` | Menus 1 and 2 | The encrypted backup |
| `seedlocker backups/<name>-qr.png` | Menu 8 | The backup as a QR code picture (encrypted, like the file) |
| `seedlocker backups/<name>-for-sparrow.txt` and `<name>-for-sparrow-qr.png` | Menu 5 | What Sparrow needs for the watch-only wallet |
| `seedlocker backups/cosigner keys/<name>-cosigner.txt` and `<name>-cosigner-qr.png` | Menu 10, 1 | This seed's key for a multisig wallet |
| `seedlocker backups/multisig wallets/<name>.multisig` | Menu 10, 2 | A registered multisig wallet |
| `<payment>.signed.psbt`, next to the payment's file | Menu 6 | The signed payment, when it came as a file |
| `seedlocker transactions/signed-<number>.psbt` | Menu 6 | The signed payment, when it came by camera |
| `seedlocker transactions/<name>.qr.html` | `sign --qr-browser` | A web page with the signed payment's QR code |

* None of these files holds a secret except the backup itself, which is encrypted. But the files for Sparrow, the
  cosigner keys and the multisig wallets show your addresses to anyone who reads them, so keep them private.
* When a name is already taken, the new file gets `-2`, `-3`... at the end of its name, for example
  `my wallet-for-sparrow-2.txt` or `tx.signed-2.psbt`. Nothing is replaced.
* `seedlocker transactions` is created next to `seedlocker backups`, in the folder you start SeedLock from.
* Earlier versions of SeedLock kept the cosigner keys and the multisig wallets directly in the backups folder; they
  are moved into their folders by themselves.

## Create a seed: menu 1

Do this in private: nobody looking, no camera pointed at the screen.

1. **The number of words:** `Number of words, 12 or 24 [24]`: press Enter for 24 words, or type 12. Both are safe;
   24 is the usual choice for long-term savings.
2. **Dice:** `Add your own dice rolls as extra randomness? [y/N]`: press Enter to let the computer choose the seed,
   or type `y` to add your own dice rolls (see [Rolling dice](#rolling-dice)).
3. **Your words:** SeedLock shows the words, numbered. Write them on paper, in this exact order, and write the
   **wallet fingerprint** shown below them next to them. Press Enter: the screen is cleared.
4. **The check:** SeedLock asks for three of the words by their number (`Word #7:`). The first four letters of a word
   are enough. If one is wrong, it shows the words again, so you can correct your paper.
5. **The name:** give the backup a name (see [Names](#names)).
6. **The password:** press Enter on the empty line, and SeedLock **generates a strong password** of 8 random words.
   Save it in your password manager exactly as shown (on another device: the offline computer has no password
   manager), then type it back, copied from the password manager, to prove that it was saved correctly. Or type a
   password of your own, twice: at least 8 characters (under 12, SeedLock asks whether you are sure). Without this
   password, the backup can never be opened.
7. **Saved and checked:** SeedLock encrypts the backup (this takes a moment), saves it, opens it again from the disk
   to prove that it decrypts to the same seed, and shows where it is saved, with the wallet fingerprint.
8. Optionally, it shows the encrypted backup as a QR code (to scan with a phone, for example). Menu 8 does the same
   and also saves it as a picture.

Then: copy the backup file to a USB stick and/or the cloud, keep the password in your password manager, and check
the backup now and then with menu 4.

### Rolling dice

Answer `y` to the dice question in menu 1. Roll a six-sided die **50 times for a 12-word seed, or 100 times for a
24-word seed**.

* Type each result as you roll it: the digits 1 to 6. What you type stays hidden, and the counter goes up the moment
  you type each roll, without Enter: `[30 of 100 rolls] >`. This works in the terminals of Linux and macOS and in the
  console windows of Windows.
* Made a mistake? Backspace removes the last roll. Anything that is not a die is explained and not counted. The last
  roll can still be changed after the counter has reached its total; then Enter continues.
* Your rolls are mixed with the computer's own randomness, so a few careless rolls cannot weaken the seed.

On the command line (see [The command line](#the-command-line)): `new --dice` does the same; `-w 15`, `-w 18` and
`-w 21` make 15, 18 and 21 words (62, 75 and 87 rolls); and `new --dice --dice-only` makes the seed from the dice
alone, exactly as SeedSigner does (the same rolls always give the same seed; 99 rolls for 24 words, as in
SeedSigner). When the rolls come from a script instead of a person, they are read line by line, as many as you like
per line; `u` removes the last line, and a wrong character stops with an error that says which roll it was.

## More about backups: menus 2, 3, 4, 7 and 8

### Menu 2: a seed you already have

Turns the words of an existing seed (from another wallet, or from your paper) into a SeedLock backup. Type the words
separated by spaces (they stay hidden), then name the backup and choose its password as in
[menu 1](#create-a-seed-menu-1). SeedLock shows the wallet fingerprint: it must be the same as in your other wallet.

### Menu 3: show the words

Choose a backup and type its password: SeedLock shows the words and the wallet fingerprint. Make sure nobody can see
the screen. Press Enter, and the screen and the terminal's scrollback are cleared.

### Menu 4: check a backup

Choose a backup. SeedLock asks `Also compare it with the words on your paper backup? [y/N]`: answer `y` to check
your paper too. Type the backup's password: SeedLock says whether the backup opens (`Backup OK: it decrypts
correctly and the mnemonic is valid.`) and shows its fingerprint, without showing the words. If you answered `y`,
type the words from your paper: SeedLock says whether they **match** this backup. Do this after making a backup, and
now and then afterwards.

### Menu 7: a strong password

Generates a password of 8 random words (about 88 bits of entropy, far more than anyone can guess), easy to type. Use
it for anything; menus 1 and 2 generate one the same way when you press Enter at the password question.

### Menu 8: a backup as a QR code

Shows a backup as a QR code in a window, so that another device can read it (a phone, for example). It holds the
**encrypted** backup only: useless without the password. SeedLock also saves it as a picture next to the backup,
`<name>-qr.png` (large and sharp, to print or to keep on a phone), and then offers to save a copy somewhere else too:
type or drop a folder, for example on a USB stick, or press Enter to skip. A file that is already there is never
replaced.

To use the backup from its QR code later, choose `c` where SeedLock asks for a backup (see
[Choosing a backup](#choosing-a-backup)).

## Set up Sparrow: menu 5

Sparrow, on the online computer, needs your wallet's **public** information to show your balance and to prepare
payments: the master fingerprint, the derivation and the xpub. Menu 5 shows them. They cannot spend anything.

### In SeedLock

1. Choose the backup, answer whether the wallet uses a passphrase, and type the backup's password.
2. **The network and the address type are found by themselves** when the folder you started SeedLock from, or
   `seedlocker transactions`, holds a transaction of this very wallet (SeedLock checks its fingerprint): the newest
   one says both. Without one, SeedLock asks one question, `Is it for real bitcoin (mainnet) or for testing (testnet
   or signet)? Type m or t [m]`, and shows Native Segwit, the usual address type.
3. SeedLock shows:
   * **Sparrow's three fields:** *Master fingerprint*, *Derivation* (written the way Sparrow's field wants it, for
     example `m/84'/0'/0'`, not the descriptor notation `84h/0h/0h`, which that field does not accept), and the
     *xpub*.
   * **The output descriptor**: the whole wallet in one line, `wpkh([fingerprint/84h/0h/0h]xpub.../<0;1>/*)#checksum`.
   * The same wallet with the other address types (Taproot, Nested Segwit, Legacy), each with its derivation and
     xpub, named as in Sparrow's *Script Type* list.
   * **The first five receive addresses.** Sparrow must show exactly these.
4. `Show the wallet as a QR code for Sparrow to scan? [Y/n]`: press Enter, and the descriptor appears as a QR code in
   a window of its own.
5. SeedLock saves all of it next to your backups: `<name>-for-sparrow.txt` and the QR code as `<name>-for-sparrow-qr.png`.

### In Sparrow

Use one of these three ways:

* **Scan the QR code:** choose **File > Import Wallet**, then **Output Descriptor**, and click **Scan...**. Hold
  Sparrow's camera (the online computer's webcam) toward SeedLock's QR window. Then press Enter in SeedLock to close
  the window.
* **Type the fields:** choose **File > New Wallet** and give the wallet a name. Keep *Policy Type* "Single
  Signature" and choose the *Script Type* that SeedLock named (usually "Native Segwit (P2WPKH)"). Under *Keystores*,
  click **Watch Only Wallet** (in Sparrow before 2.5: **xPub / Watch Only Wallet**) and type the *Master
  fingerprint*, the *Derivation* and the *xpub*. Click **Apply**.
* **Paste the descriptor:** bring `<name>-for-sparrow.txt` over on the USB stick and open it. Choose **File > New
  Wallet**, give the wallet a name, and on its **Settings** tab, next to *Descriptor*, click **Edit...** and paste
  the descriptor line. Click **Apply**.

Then open Sparrow's **Addresses** tab: the first receive addresses must be exactly the ones SeedLock showed. If they
are not, the network, the address type or the passphrase differs.

## Sign a payment: menu 6

### In Sparrow: prepare the payment

1. Open the watch-only wallet, go to the **Send** tab, fill in the address and the amount, and click **Create
   Transaction**.
2. Check it and click **Finalize Transaction for Signing**.
3. Then either click **Save Transaction** and save the `.psbt` file on a USB stick, or click **Show QR** to show it as
   a QR code (usually animated).

### In SeedLock: check and sign

1. **Which backup** holds the wallet that must sign (see [Choosing a backup](#choosing-a-backup)).
2. **How the payment arrives:**
   * **1) From a file.** SeedLock lists the `.psbt` files it finds in the folder you started it from and in
     `seedlocker transactions`. Choose one by number, or type its path, or drag it from the USB stick onto the
     terminal window and press Enter. `q` goes back to the menu.
   * **2) With the camera.** Hold the offline computer's camera toward Sparrow's QR code. A window shows what the
     camera sees: a frame appears around a QR code that was read, and a bar along the bottom fills as the payment
     arrives; the terminal shows the same progress (`4 of 12 parts received`). The code changes all the time: that
     is normal, keep the camera on it. The camera turns off as soon as the payment is complete. It needs ffmpeg (see
     [QR codes and the camera](#qr-codes-and-the-camera-menu-9)).
3. Whether the wallet uses a passphrase.
4. **The network is found by itself** from the transaction (its key paths say mainnet or a test network) and shown
   as "mainnet (real bitcoin)" or "a test network (testnet or signet)". SeedLock asks only when the transaction
   cannot tell, which should not happen with Sparrow.
5. The backup's password.
6. **The review.** SeedLock shows every input and every output, with the amounts:
   * `>>> SENT TO SOMEONE ELSE. Check this address carefully! <<<`: a payment to someone else. **Compare the address
     with the one you meant to pay**, all of it, not only the beginning and the end.
   * `change back to you (verified from your seed)`: your own change. SeedLock calls an output change only after
     working out its address from your seed, so a program that swaps the change address for someone else's is
     caught: that output is then shown as sent to someone else.
   * The total, the **fee**, the amount leaving your wallet, and any **WARNINGS**.
7. `Do you want to SIGN this transaction? [y/N]`: type `y` only if everything is what you intended. Anything else
   cancels, and nothing is signed.

### Give the signed payment back to Sparrow

SeedLock always saves the signed payment as a file first, and shows where: next to the file it came from (`tx.psbt`
becomes `tx.signed.psbt`, so a payment from the USB stick goes back on the USB stick), or in `seedlocker
transactions` when it came by camera. Then it asks:

* **1) A QR code in a window** (the default). The signed payment appears as an animated QR code in a window of its
  own, like Sparrow's. In Sparrow, on the payment's tab, click **Scan QR** and hold the online computer's camera
  toward the window. When Sparrow has it, click on SeedLock's terminal and press Enter: the window closes. The window
  can be made bigger by dragging its corner. Typing `t` instead of Enter draws the code in the terminal instead.
* **2) The file.** Take the signed file over yourself, on the USB stick. In Sparrow, on the payment's tab, click
  **Load Transaction** and choose it.

Sparrow then shows the payment as signed. Click **Broadcast Transaction** to send it to the Bitcoin network.

### What SeedLock refuses to sign

A transaction is refused, with the reason, when:

* an input does not say its amount, or a legacy input's previous transaction does not match it (otherwise the fee
  could be hidden);
* it asks for any kind of signature other than the usual one (SIGHASH_ALL);
* an input claims to be yours but is neither a plain single-signature input of your seed nor an input of a
  registered multisig wallet that rebuilds exactly its script;
* it is a multisig wallet's transaction and that wallet is not registered ([menu 10](#multisig-wallets-menu-10)).

A transaction for another network than the one found or chosen is flagged in the review.

## Multisig wallets: menu 10

A multisig wallet needs several seeds to spend: in a 2-of-3 wallet, any 2 of its 3. Sparrow keeps the wallet,
watch-only, on the online computer, and each seed signs on its own. SeedLock can hold one of the seeds, or more, each
as its own backup; the others can be on other devices (a hardware wallet, a SeedSigner, another SeedLock computer).

SeedLock supports the three kinds that Sparrow makes for *Policy Type* "Multi Signature": *Script Type* "Native
Segwit (P2WSH)" (the usual one, `wsh(...)` in its descriptor), "Nested Segwit (P2SH-P2WSH)" (`sh(wsh(...))`) and
"Legacy (P2SH)" (`sh(...)`), with the keys sorted (`sortedmulti`, Sparrow's default) or in a fixed order (`multi`).
Taproot multisig is not supported (Sparrow cannot make it either).

Menu 10 offers: **1)** share this seed's key, **2)** register a multisig wallet, **3)** show the registered
multisig wallets. Enter goes back.

### Step 1: share each seed's key (menu 10, then 1)

Do this for each SeedLock seed of the wallet.

1. Choose the backup, answer the passphrase question and type the password.
2. SeedLock asks whether the wallet is for mainnet or a test network (unless a multisig transaction of this seed is in
   the folder you started it from, or in `seedlocker transactions`), and which kind it is (Native Segwit is the usual
   one). When this seed is already in a registered wallet, it takes both from there and asks nothing.
3. It shows the key: the *Master fingerprint*, the *Derivation* and the *xpub*. The derivation follows the usual
   standards: `m/48'/0'/0'/2'` for Native Segwit and `m/48'/0'/0'/1'` for Nested Segwit on mainnet (`m/48'/1'/...`
   on a test network), and `m/45'` for Legacy. `Show this seed's key as a QR code for Sparrow to scan? [Y/n]`: press
   Enter.
4. **In Sparrow:** choose **File > New Wallet**, give it a name, choose *Policy Type* "Multi Signature", the number
   of cosigners and of signatures needed (for example 2 of 3), and the same *Script Type*. Each cosigner has a
   keystore tab. For this seed's keystore, click **Watch Only Wallet** (in Sparrow before 2.5: **xPub / Watch Only
   Wallet**), then click the **camera button** next to the *xpub* field (its tooltip says "Scan a QR code") and hold
   Sparrow's camera toward SeedLock's QR window. That fills in the three fields and names the keystore after the
   backup's file name (its first 16 characters, Sparrow's limit). Or type the three fields.
5. Fill in the other cosigners' keystores the same way (other devices show their own keys), and click **Apply** to
   save the wallet.

SeedLock saves the key as text and as a QR picture in the folder `cosigner keys` (inside `seedlocker backups`). These
files hold no secret: only this seed's public key for this kind of wallet. You do not need them to sign. Keep them to
add this seed to a wallet again later (for example, after reinstalling Sparrow), or to give this seed's key to
someone else who sets up the wallet. The registered wallet (step 2) is what SeedLock itself uses.

### Step 2: register the wallet (menu 10, then 2)

SeedLock must know the whole wallet, all its cosigners, before it signs its payments: that is how it checks every
input and the change.

1. **In Sparrow,** open the multisig wallet and go to its **Settings** tab. Next to *Descriptor* (under *Script
   Policy*), click the **QR code button** ("Show as QR code"). Or save the descriptor as a file: **File > Export
   Wallet**, then **Output Descriptor**.
2. **In SeedLock,** at `Type or drop that file and press Enter, paste the descriptor, or type c to scan Sparrow's QR
   code with the camera`: type `c` and hold the camera toward Sparrow's QR code (animated or not, in UR or in BBQr
   format, see [Sparrow's QR codes](#sparrows-qr-codes)). Or drop the exported file onto the window and press
   Enter, or paste the descriptor.
3. SeedLock checks the descriptor (its checksum and its keys; it never accepts a private key), shows the cosigners
   and **the wallet's first receive address**. Sparrow must show exactly this one (the wallet's **Addresses** tab,
   the first receive address). Answer `y` if it matches and the cosigners are right.
4. Give the wallet a name (Enter keeps `multisig-2-of-3`). SeedLock saves it as `<name>.multisig` in the folder
   `multisig wallets` (inside `seedlocker backups`).

Register each wallet once on each SeedLock computer that holds one of its seeds. Menu 10, 3 lists the registered
wallets with their cosigners and first address.

### Step 3: sign (menu 6)

Sign as with any wallet ([menu 6](#sign-a-payment-menu-6)). SeedLock signs an input only if the registered wallet
rebuilds **exactly** its script from all the cosigners' keys, this seed's among them. An output is called change
only if its script is rebuilt the same way, so a "change" output with someone else's keys in it is shown as a payment
to someone else. The review says which cosigner this seed is, and how many signatures the payment has and needs:

```
Multisig "family vault", 2 of 3: this seed is cosigner 1 and adds its signature (1 of the 2 needed, with it).
1 more cosigner must sign it too before Sparrow can finalize it.
```

Then take the signed payment to the next cosigner, as a file or as a QR code, or back to Sparrow, which collects the
signatures (Sparrow can also show it as a QR code again for the next cosigner). When there are enough, Sparrow
finalizes it and **Broadcast Transaction** sends it.

When this computer holds two seeds of the same wallet, sign twice in menu 6: first with one backup, then with the
other, choosing the file that the first signature made (`tx.signed.psbt`). The list in menu 6 shows only unsigned
files, so type or drop that file's path.

## QR codes and the camera: menu 9

### ffmpeg

SeedLock reads the camera, and shows its own QR codes in a window, with the free program **ffmpeg** (and **ffplay**,
which comes with it). They run as separate programs, so no camera code is built into SeedLock. Without them, SeedLock
works with files on a USB stick, and draws its QR codes in the terminal.

| System | How to get ffmpeg |
|---|---|
| Debian, Ubuntu, Lubuntu | `sudo apt install ffmpeg`, while the computer is still online |
| Arch | `sudo pacman -S ffmpeg`, while the computer is still online (`xdg-utils` too, only for `--qr-browser`) |
| macOS | `brew install ffmpeg` with [Homebrew](https://brew.sh), while the computer is still online. Allow your terminal app to use the camera: System Settings > Privacy & Security > Camera |
| Windows | Put `ffmpeg.exe` and `ffplay.exe` **in the same folder as `seedlock.exe`**: download `ffmpeg-release-essentials.zip` (on [ffmpeg.org](https://ffmpeg.org/download.html), under Windows, the builds from gyan.dev) on the online computer, bring it over on the USB stick, and copy the two files from its `bin` folder. On a computer that is online, `winget install ffmpeg` works too. Allow desktop apps to use the camera: Settings > Privacy & security > Camera |

If ffmpeg is installed but SeedLock does not find it, set the environment variable `SEEDLOCK_FFMPEG` to its full path
(see [Settings](#settings-environment-variables)).

**On Linux** the first camera is `/dev/video0`. If you see `Permission denied`, add yourself to the `video` group
(`sudo usermod -aG video $USER`), then log out and in again. `v4l2-ctl --list-devices` (package `v4l-utils`) shows
which number each camera has; choose another one with `--camera N` on the command line (for example
`sign "my wallet" --scan --camera 1`). A camera that another program is using cannot be opened.

### Menu 9: test the camera

Opens the camera and a window that shows what it sees, and waits up to 20 seconds for a QR code (Sparrow's animated
code will do). Then it says how many pictures arrived, and whether a QR code was read. Run it once to check your
setup. (On the command line: `camera`, or `camera --camera N`.)

SeedLock asks the camera for 1280 x 720 pictures, or 640 x 480 when it cannot. It looks at each picture as it is, then
enlarged, sharpened, reduced, and with its light evened out, which reads far more pictures from a soft or noisy
webcam.

### Scanning tips

* Make Sparrow's QR code as large as possible on its screen.
* Hold the camera steady, about an arm's length away or closer, so that the code fills a good part of the camera's
  picture. Avoid glare and reflections on the screen.
* **The preview window** shows what the camera sees. A frame appears around a code that was read. Closing the window
  does not stop the scan. Without a graphical screen, or without ffplay, SeedLock says so and scans without it.
* If SeedLock sees a QR code for a few seconds but cannot read it, it says so, and what helps.
* Ctrl+C gives up. SeedLock gives up by itself after 3 minutes.

### Sparrow's QR codes

Sparrow's QR window has two settings, and SeedLock reads every combination:

* **The format: UR or BBQr.** UR is Sparrow's default; BBQr is Coinkite's format (also used by Coldcard). In Sparrow
  2.4 and 2.5 the format is a list at the bottom of the QR window. Sparrow remembers the choice. SeedLock reads both
  wherever it scans: a payment (menu 6) and a multisig wallet's descriptor (menu 10, 2). The codes SeedLock shows
  Sparrow are UR (the signed payment, a cosigner key) or plain text (the wallet's descriptor in menu 5), never BBQr,
  and Sparrow reads them whatever its own setting.
* **The density.** A low density makes small QR codes and shows a larger payment as an animation of several codes.
  A higher density allows much larger codes, so a whole multisig wallet, or a small payment, becomes **one large,
  dense QR code**. A dense code is harder for a webcam to read from a screen: the screen's own pixel pattern, glare
  and soft focus cost more when its squares are small. **If SeedLock does not read a code, make it less dense**: in
  Sparrow 2.4 and 2.5, click **Less Dense** at the bottom of the QR window (in older versions **Decrease Density**;
  in newer ones, set the density list to **Low**). Small animated codes read most easily.

### SeedLock's QR window

SeedLock shows its own QR codes (the signed payment, a backup in menu 8, the wallet for Sparrow in menu 5, a cosigner
key in menu 10) in a window of its own, about 640 pixels square, the way Sparrow shows its codes. Its size does not
depend on the terminal. An animated code changes about three times a second.

* Make it bigger by dragging its corner. If you do not see it, it may be behind the terminal.
* Press Enter in the terminal when you are done: the window closes. It also closes if SeedLock stops for any reason.
* Type `t` instead of Enter to draw the code in the terminal instead.
* Without a graphical screen or without ffplay, SeedLock says why in one line and draws the code in the terminal.

### A QR code in the terminal

When the code is drawn in the terminal (typing `t`, `--qr-terminal`, or when no window can be opened), it needs a
window of about 54 columns by 32 lines. SeedLock checks the size first and asks you to maximize the window if needed;
if that is still not enough, zoom out with Ctrl and - (Cmd and - on a Mac), a few times, and press Enter.

**Apple's Terminal app** draws the code twice as large (about 107 columns by 58 lines for a typical payment): a
smaller code drawn there cannot be read by a camera, because that app leaves thin white lines through the black
parts. A box explains this first. iTerm2 and the terminals on Linux and Windows use the smaller code. `SEEDLOCK_QR_STYLE`
changes this (see [Settings](#settings-environment-variables)).

### A QR code in the web browser

`sign ... --qr-browser` shows the signed payment's QR code as a large page in your web browser instead, for screens
where the window cannot be made big enough (press F11 for full screen). The page works offline and is saved as
`seedlocker transactions/<name>.qr.html`; at its "press Enter" step, `t` draws the code in the terminal after all.
On Linux the browser is started with `xdg-open` (package `xdg-utils`), which needs a default browser, for example
`xdg-settings set default-web-browser firefox.desktop`; without one, SeedLock says where the page is saved and draws
the code in the terminal.

### Settings (environment variables)

These are set in the terminal before starting SeedLock, for example `SEEDLOCK_NO_PREVIEW=1 ./seedlock` on Linux and
macOS, or `$env:SEEDLOCK_NO_PREVIEW=1` (Enter), then `.\seedlock.exe`, in PowerShell.

| Setting | What it does |
|---|---|
| `SEEDLOCK_FFMPEG=/path/to/ffmpeg` | Uses this ffmpeg |
| `SEEDLOCK_FFPLAY=/path/to/ffplay` | Uses this ffplay (normally the one next to ffmpeg) |
| `SEEDLOCK_NO_PREVIEW=1` | Scans without the preview window |
| `SEEDLOCK_NO_QR_WINDOW=1` | Draws QR codes in the terminal instead of a window |
| `SEEDLOCK_QR_STYLE=full` or `compact` | The large or the small QR code in the terminal (normally large only in Apple's Terminal). `full` helps where the terminal cannot be recognised, for example inside tmux or over SSH |

## Security advice

**The backup is exactly as strong as its password.** It protects your seed against whoever gets hold of the file (a
lost USB stick, a hacked cloud account). SeedLock makes every password guess slow (Argon2id, 256 MiB of memory per
guess), but that cannot save a weak password. Use the generated password, or a long random one.

**Keep the password apart from the file.** If one account holds both, one break-in loses both. Use a password manager
with its own strong master password and two-factor login, separate from where the backup files are stored.

**Keep a paper backup too:** the words, on paper or metal, kept safe. Check it against the backup with menu 4 (answer
`y` to the comparison). A digital backup is convenient, not indestructible.

**A passphrase is never stored.** If you use one, only you know it: SeedLock asks for it when you sign, but keeps it
nowhere. Lose it and the funds are gone.

**A computer is not a hardware signing device.** A SeedSigner or a hardware wallet has no operating system, network
or storage to attack; on an ordinary computer, malicious software could read a seed while it is open. So:

* use a computer that never goes online again (best of all, one with a freshly installed system, or a system started
  from a USB stick, a "live" system), and treat it as sensitive;
* move files with a USB stick used only for this, or use QR codes, which carry nothing else;
* consider turning off swap and hibernation: SeedLock wipes secrets from its memory as soon as it can, but it cannot
  control what the system writes to disk.

SeedLock itself never touches the network, clears the screen and the scrollback after showing words, reads secrets
only from hidden prompts, never from the command line, and on Linux and macOS does not let a crash write its memory
to disk.

**Check every payment on SeedLock's screen** before you sign: the addresses that receive money, and the fee. The
online computer could show you something else than what it sends to SeedLock; SeedLock's review is what counts.

### How strong the encryption is

The memory each password guess costs is chosen when the backup is made, and stored in it:

| Setting | Memory per guess | Time to open the backup\* |
|---|---|---|
| `light` | 64 MiB | 0.1 s |
| `standard` (menus 1 and 2) | 256 MiB | 0.5 s |
| `strong` | 1 GiB | 2.4 s |

\* Measured on a single-core test computer. The command line chooses with `--kdf`, for example
`new --kdf strong`; use `light` for a backup that must also open on a very old phone. `inspect NAME` shows a backup's
setting without its password.

## The command line

The menu does everything most people need. Every task is also a command, for scripts and for more options.

**How to type a command on the offline computer:** in SeedLock's folder, type the program as you start it, then the
command:

* Linux and macOS: `./seedlock sign "my wallet" --psbt tx.psbt`
* Windows, in PowerShell: `.\seedlock.exe sign "my wallet" --psbt tx.psbt`

In the table below, type `./seedlock` (or `.\seedlock.exe`) where it says `seedlock`. `./seedlock --help` lists
the commands, and `./seedlock sign --help` (or any other command) explains one.

| Command | What it does |
|---|---|
| `seedlock` | The menu |
| `seedlock new [-w 12\|15\|18\|21\|24] [--dice [--dice-only]]` | A new seed, straight into an encrypted backup (menu 1) |
| `seedlock encrypt` | A backup of a seed you already have (menu 2) |
| `seedlock decrypt NAME` | Shows the words (menu 3). `--raw` prints only the words, for scripts |
| `seedlock verify NAME [--fingerprint HEX] [--compare-words]` | Checks that a backup opens, without showing it (menu 4); optionally against a fingerprint or your paper words |
| `seedlock info NAME [--network ...] [--script wpkh\|tr\|sh-wpkh\|pkh] [--passphrase]` | Fingerprint, xpub, descriptors and addresses (`--account`, `--change`, `--start`, `--count`) |
| `seedlock sign NAME (--psbt FILE \| --scan [--camera N]) [--network ...] [--passphrase] [--qr \| --qr-terminal \| --qr-browser \| --static-qr]` | Reviews and signs a payment from a file or the camera (menu 6) |
| `seedlock inspect NAME` | A backup's encryption settings (no password needed) |
| `seedlock qr NAME [--svg FILE]` | A backup as a QR code, also saved as `NAME-qr.png` in the backups folder (menu 8) |
| `seedlock camera [--camera N]` | Tests the camera (menu 9) |
| `seedlock password [-w N]` | A random password of N words (menu 7) |
| `seedlock final-word` | The valid last words for a seed whose other words you chose yourself (11, 14, 17, 20 or 23 words) |

* **`NAME`** is the name of a backup in `seedlocker backups` (with or without `.seedlock`), the path of a `.seedlock`
  file, or the backup's `SEEDLOCK1:...` text itself. Put a name with spaces in quotes: `"my wallet"`.
* `new` and `encrypt` take `--out FILE` to save the backup at an exact path instead of the backups folder, and
  `--kdf light|standard|strong`. `sign` takes `--out FILE` for the signed payment. An existing file is never
  overwritten.
* `--passphrase` makes `info` and `sign` ask for the wallet's passphrase.
* `info` also prints the account's xpub with its key origin (`[fingerprint/84h/0h/0h]xpub...`) and the receive and
  change descriptors with their checksums, for other wallets such as Nunchuk, Specter or Bitcoin Core
  (`importdescriptors`).
* `--network mainnet|testnet|signet|regtest`: `info` shows mainnet unless told otherwise; `sign` finds the network
  by itself. Testnet, signet and regtest use the same transactions, so a payment cannot say which of them it is;
  only regtest writes its addresses differently (`bcrt1...`), so use `sign --network regtest` for it.
* After signing, `--qr` shows the signed payment's QR code (in the window), `--qr-terminal` draws it in the terminal,
  `--qr-browser` shows it in the web browser, and `--static-qr` shows one single dense code instead of an animated one
  (small payments only; the animated code is easier to read). Without a terminal, `--qr` prints the code's parts as
  text (`ur:crypto-psbt/...`, one per line).
* Secrets (words, passwords, the passphrase, dice rolls) are only read from hidden prompts, or from the input when it
  comes from a script, never from the command itself, so they do not end up in the terminal's history.
* For scripts: `verify` ends with exit code 2 when a comparison fails, and SeedLock ends with exit code 1 when
  something goes wrong.

Examples:

```sh
./seedlock sign "my wallet" --psbt /media/me/USB/tx.psbt     # a payment from the USB stick
./seedlock sign "my wallet" --scan                           # a payment scanned with the camera
./seedlock verify "my wallet" --compare-words                # check the backup against your paper words
./seedlock info "my wallet" --script tr                      # the Taproot addresses of the same wallet
./seedlock new --dice --dice-only -w 12                      # 12 words from 50 dice rolls alone
```

## Opening a backup without SeedLock

Your backups do not depend on this program. Their format is documented byte for byte in
[FORMAT.md](FORMAT.md) (Argon2id and XChaCha20-Poly1305, both standard), and
[`tools/recover.py`](../tools/recover.py) is a separate, short Python program that opens a backup without SeedLock.
Like SeedLock, it shows your words, so run it only on an offline computer:

1. Copy `recover.py` (in the `tools` folder of SeedLock's download) to the offline computer, into the folder that
   holds `seedlocker backups` (SeedLock's folder).
2. While that computer is still online, open a terminal in that folder and install the three Python libraries
   `recover.py` needs: `pip install argon2-cffi pynacl mnemonic`. Recent Debian and Ubuntu systems do not let pip
   change the system's Python; there, make a separate one in that folder first, `python3 -m venv recover-env`, then
   type `recover-env/bin/pip install argon2-cffi pynacl mnemonic`, and in step 3 type `recover-env/bin/python3`
   instead of `python3`.
3. Offline, in a terminal in that folder, type `python3 recover.py "seedlocker backups/my wallet.seedlock"`. It asks
   for the password and prints the words.
