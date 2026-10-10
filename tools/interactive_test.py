#!/usr/bin/env python3
"""
Drives SeedLock in a real pseudo-terminal, exactly like a person would.

Checks: the backups folder, the "name your backup" prompt (and that a taken name asks again
instead of aborting), the backup picker in the menu, no overwriting and no "--force" anywhere,
the dice counter (typo, undo, 50/100 rolls), that typed secrets are never echoed, that Ctrl+C at a
hidden prompt puts the terminal back, the online and virtual machine warnings, and the QR
codes for signing (terminal fallback): the animated QR is captured from the screen, decoded with OpenCV and
reassembled with the independent Foundation Devices UR library; the static QR is decoded and
parsed by embit.

    pip install pexpect opencv-python-headless numpy cbor2 embit mnemonic pyte
    git clone https://github.com/Foundation-Devices/foundation-ur-py
    python3 interactive_test.py ../target/release/seedlock --ur-lib foundation-ur-py
"""
import argparse, io, os, re, sys, tempfile, time

import cbor2, cv2, numpy as np, pexpect, pyte
from embit import bip32, bip39, script
from embit.psbt import PSBT, DerivationPath
from embit.transaction import Transaction, TransactionInput, TransactionOutput

ap = argparse.ArgumentParser()
ap.add_argument("bin", nargs="?", default="seedlock")
ap.add_argument("--ur-lib", default=os.environ.get("FOUNDATION_UR_PY"))
args = ap.parse_args()
BIN = os.path.abspath(args.bin) if os.path.exists(args.bin) else args.bin
if args.ur_lib:
    sys.path.insert(0, os.path.abspath(args.ur_lib))
try:
    from ur.ur_decoder import URDecoder
except Exception:
    URDecoder = None

# No browser is started by these tests: the QR code is drawn in the terminal (see qr_flow_test.py for the browser page).
os.environ["SEEDLOCK_NO_BROWSER"] = "1"
for inherited in ("TERM_PROGRAM", "SEEDLOCK_QR_STYLE"):   # the QR style follows these: tests set them explicitly
    os.environ.pop(inherited, None)
os.environ["SEEDLOCK_NO_QR_WINDOW"] = "1"   # this script tests the drawing in the terminal; the QR window is tested in qr_flow_test.py

# Stand-in routing tables (the online check reads /proc/net on Linux): offline unless a test says otherwise.
NET = tempfile.mkdtemp()
_ROUTE_HEAD = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n"
_V6_LO = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo\n"
for _state, _row in (("offline", "docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n"),
                     ("online", "wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n")):
    os.makedirs(os.path.join(NET, _state))
    open(os.path.join(NET, _state, "route"), "w").write(_ROUTE_HEAD + _row)
    open(os.path.join(NET, _state, "ipv6_route"), "w").write(_V6_LO)
os.environ["SEEDLOCK_TEST_NET_DIR"] = os.path.join(NET, "offline")
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
CSI = re.compile(r"\x1b\[([0-9;?]*)([A-Za-z])")
NAME_PROMPT = "Save the encrypted backup as [give-a-name-to-your-backup]: "
work = tempfile.mkdtemp()
ALL_LOGS = io.StringIO()


class Session:
    def __init__(self, cli_args=(), env=None, dims=(70, 150)):
        self.log = io.StringIO()
        self.c = pexpect.spawn(BIN, list(cli_args), encoding="utf-8", timeout=120, dimensions=dims, cwd=work, env=env)
        self.c.logfile_read = self

    def write(self, s):
        self.log.write(s); ALL_LOGS.write(s)

    def flush(self):
        pass

    def expect(self, pattern, **kw):
        return self.c.expect(pattern, **kw)

    def text(self, exact):
        return self.c.expect(re.escape(exact))

    def send(self, s=""):
        self.c.sendline(s)

    def __getattr__(self, name):
        return getattr(self.c, name)


def grid_words(text):
    found = re.findall(r"(?<!\d)(\d{1,2})\. ([a-z]+)", text)
    return [w for _, w in sorted((int(n), w) for n, w in found)]


def qr_text(block_lines):
    width = max(len(l) for l in block_lines)
    rows = []
    for l in block_lines:
        l = l.ljust(width)
        rows.append([1 if ch in "\u2580\u2588" else 0 for ch in l])   # upper half dark
        rows.append([1 if ch in "\u2584\u2588" else 0 for ch in l])   # lower half dark
    img = np.where(np.array(rows, dtype=np.uint8) == 1, 0, 255).astype(np.uint8)
    img = cv2.resize(img, None, fx=8, fy=8, interpolation=cv2.INTER_NEAREST)
    # The Aruco-based detector reads big (dense) codes better; the classic one is the fallback.
    detectors = ([cv2.QRCodeDetectorAruco] if hasattr(cv2, "QRCodeDetectorAruco") else []) + [cv2.QRCodeDetector]
    for make in detectors:
        text, _, _ = make().detectAndDecode(img)
        if text:
            return text
    return ""


def qr_lines(chunk):
    lines = [ANSI.sub("", l).rstrip("\r") for l in chunk.split("\n")]
    return [l for l in lines if len(l) > 20 and set(l) <= set(" \u2580\u2584\u2588")]


def full_grid(chunk):
    """A code drawn in the full style (Apple's Terminal), read back from the raw terminal output into rows
    of squares (1 = dark), checking that it uses no block characters and that every square is two spaces
    of one background colour."""
    grid = []
    for line in chunk.split("\n"):
        cells, background, i = [], None, 0
        while i < len(line):
            m = CSI.match(line, i)
            if m:
                params, command = m.groups()
                if command == "m":
                    background = {"48;5;16": 1, "48;5;231": 0}.get(params, None if params in ("", "0") else background)
                i = m.end()
                continue
            assert line[i] not in "\u2580\u2584\u2588", "the full style must not use block characters"
            if line[i] == " " and background is not None:
                cells.append(background)
            i += 1
        if cells:
            assert len(cells) % 2 == 0 and cells[0::2] == cells[1::2], "every square is two spaces of one colour"
            grid.append(cells[0::2])
    return grid


def grid_text(grid):
    img = np.where(np.array(grid, dtype=np.uint8) == 1, 0, 255).astype(np.uint8)
    img = cv2.resize(img, None, fx=8, fy=8, interpolation=cv2.INTER_NEAREST)
    detectors = ([cv2.QRCodeDetectorAruco] if hasattr(cv2, "QRCodeDetectorAruco") else []) + [cv2.QRCodeDetector]
    for make in detectors:
        text, _, _ = make().detectAndDecode(img)
        if text:
            return text
    return ""


def finish_backup_manual(s, name, password):
    s.text(NAME_PROMPT); s.send(name)
    s.expect(r"empty = generate one\): "); s.send(password)
    s.expect(r"Repeat the password: "); s.send(password)
    s.expect("Done. Your encrypted backup is saved and verified:")
    s.expect(r"QR code .*\[y/N\] "); s.send("n")


def type_three_words(s, words):
    for _ in range(3):
        s.expect(r"Word #(\d+): ")
        s.send(words[int(s.match.group(1)) - 1])


# ---------------------------------------------------------------- A) new seed, folder, name prompt
s = Session()
s.expect(r"\n> "); s.send("1")
s.expect(r"\[24\]: "); s.send("12")
s.expect(r"\[y/N\] "); s.send("n")
s.expect("Press Enter to continue"); s.send()
s.expect("Press Enter once every word is written down")
words1 = grid_words(s.before); assert len(words1) == 12, words1
s.send()
type_three_words(s, words1)
s.text("Your backups are kept in this folder")
s.text(NAME_PROMPT); s.send("")                                   # nothing typed: asks again
s.expect("Please type a name for your backup")
s.text(NAME_PROMPT); s.send("bad/name")                           # not allowed in a file name
s.expect("cannot contain the character '/'")
s.text(NAME_PROMPT); s.send("wallet-one")
s.expect(r"empty = generate one\): "); s.send("")                 # generate a password
s.expect(r"Type the password \(hidden\): ")
pw1 = re.search(r"([a-z]+(?:-[a-z]+){7})", s.before).group(1)
s.send(pw1)
s.expect("Done. Your encrypted backup is saved and verified:")
s.expect(r"QR code .*\[y/N\] "); s.send("n")
folder = os.path.join(work, "seedlocker backups")
assert os.path.isfile(os.path.join(folder, "wallet-one.seedlock")), os.listdir(work)
assert "seedlocker backups" in s.log.getvalue()
print("A) new seed: backup lands in 'seedlocker backups'; empty/invalid names are refused politely: OK")

# ---------------------------------------------------------------- B) a taken name asks again (nothing is lost)
M2 = "legal winner thank year wave sausage worth useful legal winner thank yellow"
PW2 = "Tr0ub4dor-and-3-horses"
s.expect(r"\n> "); s.send("2")
s.expect(r"hidden as you type\):"); s.send(M2)
s.expect("Mnemonic accepted")
s.text(NAME_PROMPT); s.send("wallet-one")                          # already taken
s.expect("A backup called \"wallet-one\" already exists")
s.expect("please choose a different name")
s.text(NAME_PROMPT); s.send("Wallet-One.seedlock")                # only the case differs: still "taken" (USB sticks and clouds ignore case)
s.expect("differ only by upper or lower case count as the same")
s.text(NAME_PROMPT); s.send("wallet-two.seedlock")                # typed with the extension
s.expect(r"empty = generate one\): "); s.send(PW2)
s.expect(r"Repeat the password: "); s.send(PW2)
s.expect("Done. Your encrypted backup is saved and verified:")
s.expect(r"QR code .*\[y/N\] "); s.send("n")
second = "wallet-two"
assert sorted(f[:-9] for f in os.listdir(folder)) == ["wallet-one", second], os.listdir(folder)
print("B) a taken name (even differing only by case) asks again instead of aborting; typed extension handled: OK")

# ---------------------------------------------------------------- C) picker in the menu
CHOOSE = 'Choose a backup: number 1-2, type a name or path, or drop your backup file and press "Enter": '
s.expect(r"\n> ")
assert "4) Check if the backup opens (verify)" in s.before and "8) Generate a backup as a QR code" in s.before, s.before[-900:]
s.send("3")
s.text(CHOOSE)
listing = s.before
assert "1) wallet-one" in listing and "2) wallet-two" in listing, listing
assert "c) Scan a backup's QR code with the camera" in listing, listing
s.send("1")
s.expect(r"Backup password \(hidden\): "); s.send(pw1)
s.expect("Press Enter to clear the screen")
assert grid_words(s.before) == words1
s.send()
s.expect(r"\n> "); s.send("4")
s.text(CHOOSE); s.send(second)                                      # by typing the name
s.expect(r"paper backup\? \[y/N\] "); s.send("n")
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect("Backup OK")
# ... and by dropping the file onto the window, the way each terminal pastes it (the folder name has a space)
import urllib.parse
dropped = os.path.join(folder, second + ".seedlock")
for how, pasted in (("a Linux terminal: in quotes", f"'{dropped}' "),
                    ("Apple's Terminal: spaces escaped", dropped.replace(" ", "\\ ") + " "),
                    ("a file:// address", "file://" + urllib.parse.quote(dropped))):
    s.expect(r"\n> "); s.send("4")
    s.text(CHOOSE); s.send(pasted)
    s.expect(r"paper backup\? \[y/N\] ", timeout=10); s.send("n")
    s.expect(r"Backup password \(hidden\): "); s.send(PW2)
    s.expect("Backup OK")
print("C) the menu shows the new names; it lists the backups by number with a camera choice; choosing by number, by typed name, "
      "and by a dropped file (in quotes, with escaped spaces, as a file:// address) works: OK")

# ---------------------------------------------------------------- D) dice: the counter moves at the moment a roll is typed
s.expect(r"\n> "); s.send("1")
s.expect(r"\[24\]: "); s.send("24")
s.expect(r"\[y/N\] "); s.send("y")
s.expect("Press Enter to continue"); s.send()
s.text("Roll a six-sided die 50 times for a 12-word seed, or 100 times for a 24-word seed.")
s.text("You chose 24 words, so you need 100 rolls.")
s.text("The counter goes up with every roll you type.")
s.text("Press Backspace to remove the last roll.")
s.text("[0 of 100 rolls] > ")
s.c.send("3"); s.text("[1 of 100 rolls] > ")                        # one roll typed, no Enter: the counter has moved already
s.c.send("x"); s.text("(only the digits 1 to 6 count)")              # not a die: explained, not counted
s.c.send("\x7f"); s.text("[0 of 100 rolls] > ")                      # Backspace removes the last roll
s.c.send("3" * 30); s.text("[30 of 100 rolls] > ")
s.c.send("1 2 3 4 5 6 " * 5); s.text("[60 of 100 rolls] > ")         # spaces are fine
s.c.send("\x1b[1;5C"); s.expect(pexpect.TIMEOUT, timeout=0.4)        # an arrow key: the digits inside it are not rolls
s.c.send("6" * 30); s.text("[90 of 100 rolls] > ")
s.c.send("5" * 10); s.text("[100 of 100 rolls] Done. Press Enter to continue (Backspace changes the last roll).")
s.c.send("\x7f"); s.text("[99 of 100 rolls] > ")                      # the last roll can still be changed
s.c.send("5"); s.text("[100 of 100 rolls] Done.")
s.send(); s.text("Thank you!")
s.expect("Press Enter once every word is written down")
words24 = grid_words(s.before); assert len(words24) == 24, words24
s.send()
type_three_words(s, words24)
finish_backup_manual(s, "wallet-dice", "Another-Long-Password-9")
print("D) dice: the counter moves with every key (no Enter), a non-die is explained, Backspace removes a roll, an arrow key is not counted, the last roll can still be changed: OK")

# ---------------------------------------------------------------- E) quit
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)

# ---------------------------------------------------------------- F) sign: animated and static QR codes
mn = M2
root = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(mn))
H = bip32.HARDENED_INDEX
in_path, ch_path = [84 + H, H, H, 0, 3], [84 + H, H, H, 1, 7]
in_pub, ch_pub = root.derive(in_path).key.get_public_key(), root.derive(ch_path).key.get_public_key()
tx = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex("aa" * 32), 1)],
                 vout=[TransactionOutput(60_000, script.address_to_scriptpubkey("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")),
                       TransactionOutput(39_000, script.p2wpkh(ch_pub))])
psbt = PSBT(tx)
psbt.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wpkh(in_pub))
psbt.inputs[0].bip32_derivations[in_pub] = DerivationPath(root.my_fingerprint, in_path)
psbt.outputs[1].bip32_derivations[ch_pub] = DerivationPath(root.my_fingerprint, ch_path)
open(os.path.join(work, "send.psbt"), "w").write(psbt.to_string())


def sign_session(extra=(), env=None, dims=(70, 150)):
    s = Session(["sign", second, "--psbt", "send.psbt", *extra], env=env, dims=dims)
    s.expect(r"Backup password \(hidden\): "); s.send(PW2)
    s.expect(r"Do you want to SIGN this transaction\? \[y/N\] ")
    assert "SENT TO SOMEONE ELSE" in s.before and "change back to you" in s.before
    s.send("y")
    return s


s = sign_session()
s.expect(r"How do you want to give the signed transaction back to Sparrow\?")
s.expect(r"Choose 1 or 2 \[1\]: "); s.send()                        # Enter = 1 = show the QR code
s.expect(r"Frame 1/(\d+)"); total = int(s.match.group(1))
s.expect(pexpect.TIMEOUT, timeout=0.7)                              # keep reading, like a real terminal does
s.c.send("xyz")                                                      # stray typing must not shift the picture
s.expect(pexpect.TIMEOUT, timeout=0.3 * total * 2.4)                 # about two and a half cycles
s.expect(r"Frame (\d+)/\d+ - press Enter when done")                 # wait for a complete frame ...
snapshot = s.log.getvalue()                                          # ... and look at the screen as a person sees it
s.send(); s.expect(pexpect.EOF)
out = s.log.getvalue()

# What is on the screen must be exactly the frame that was drawn last: no drift, no leftovers, no scrolling.
screen = pyte.Screen(150, 70); pyte.Stream(screen).feed(snapshot)
assert any("Point the camera" in row for row in screen.display), "the screen scrolled: the intro text is gone"
BLOCKS = set("\u2580\u2584\u2588")
shown = [row.rstrip() for row in screen.display if set(row) & BLOCKS]
drawn = [l.rstrip() for l in qr_lines(re.split(r"\x1b\[(\d+)A", snapshot)[-1]) if set(l) & BLOCKS]
assert shown == drawn and len(shown) > 15, "the picture on the screen is not the frame that was drawn (it drifted or has leftovers)"
print(f"F0) the terminal animation redraws in place on a real (emulated) screen, even after stray typing: {len(shown)} QR rows identical to the last frame: OK")
# Wrapped in "synchronized output" (\x1b[?2026h ... \x1b[?2026l): a terminal feature that stops a
# fast camera from ever catching the screen half old frame, half new (a real terminal that has not
# heard of it just ignores the bytes; pyte, used above, does the same, which is why F0 still passed).
opens, closes = out.count("\x1b[?2026h"), out.count("\x1b[?2026l")
assert opens == closes >= total, f"{opens} opens, {closes} closes, expected at least {total} of each"
one = out[out.index("\x1b[?2026h") : out.index("\x1b[?2026l") + len("\x1b[?2026l")]
assert set(one) & BLOCKS, "a synchronized block must actually contain a drawn frame, not be empty or decorative"
print(f"F0b) every redraw is wrapped in 'synchronized output' ({opens} times): a fast camera cannot catch it half old, half new: OK")
# Black and white are drawn with the 256-colour palette's fixed entries (16, 231), not the 8-colour
# codes a custom terminal theme can repaint as something softer than pure black and white.
assert "\x1b[38;5;16;48;5;231m" in out and "\x1b[30;47m" not in out, "the QR must use the fixed, theme-proof black/white codes"
print("F0c) the QR is drawn in the theme-proof black (256-colour 16) and white (256-colour 231), not the theme-remappable 8-colour codes: OK")
assert "built-in Terminal app" not in out, "the Apple Terminal note must not appear for an ordinary terminal"
print("F0d) no Apple Terminal note appears here, since TERM_PROGRAM is not set to it: OK")
body = out[out.index("Point the camera"):]
parts = re.split(r"\x1b\[(\d+)A", body)
frames = []
for chunk in parts[0::2]:
    lines = qr_lines(chunk)
    if len(lines) >= 15:
        t = qr_text(lines)
        if t and t not in frames:
            frames.append(t)
assert len(frames) >= total // 2, f"only {len(frames)} distinct frames decoded of {total}"
assert all(f.startswith("UR:CRYPTO-PSBT/") for f in frames), frames[:2]
signed_file = os.path.join(work, "send.signed.psbt")
signed_bytes = open(signed_file, "rb").read()
if URDecoder is None:
    print(f"F1) animated QR: {len(frames)} frames decoded from the screen with OpenCV (reference UR decoder not available: skipped)")
else:
    dec = URDecoder()
    for f in frames:
        dec.receive_part(f.lower())
    assert dec.is_complete(), "the reference UR decoder could not reassemble the transaction"
    assert cbor2.loads(bytes(dec.result_message().cbor)) == signed_bytes, "decoded PSBT differs from the signed file"
    print(f"F1) animated QR: {len(frames)} distinct frames decoded from the screen, reassembled by the Foundation UR library == signed PSBT: OK")

s = sign_session(["--static-qr"]); s.expect(pexpect.EOF)   # the static code is an option for scripts and small transactions
out = s.log.getvalue()
static = qr_text(qr_lines(out[out.index("The signed PSBT is saved as"):]))
signed2 = PSBT.from_string(static)                                   # what Sparrow does with a plain-text QR
assert bytes(signed2.serialize()) == open(os.path.join(work, "send.signed-2.psbt"), "rb").read()
assert signed2.inputs[0].partial_sigs, "no signature in the static QR"
print("F2) static QR (--static-qr) decodes to the base64 PSBT (parsed by embit) == the second signed file (never overwritten): OK")

# A window that is too small: the tool asks to maximize it instead of drawing a broken animation.
s = Session(["sign", second, "--psbt", "send.psbt"])
s.c.setwinsize(24, 80)
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.expect(r"Choose 1 or 2 \[1\]: "); s.send()
s.expect(r"Your window is 80 columns x 24 lines\. The animated QR code needs at least (\d+) columns x (\d+) lines")
need_cols, need_lines = int(s.match.group(1)), int(s.match.group(2))
s.text("Maximize this window first, then press Enter to show the QR code. Type q and press Enter to skip.")
s.expect(r"> "); s.send()                                             # Enter without doing anything: still too small, and the hint gets more helpful
s.expect(r"Your window is 80 columns x 24 lines\. The animated QR code needs at least")
s.text("It is still too small. If the window is already maximized, zoom out with Ctrl and - (a few times), then press Enter. Type q and press Enter to skip.")
s.c.setwinsize(need_lines + 5, need_cols + 5)                         # maximize the window, then press Enter
s.expect(r"> "); s.send()
s.expect(r"Frame 1/\d+"); s.expect(pexpect.TIMEOUT, timeout=0.8); s.send(); s.expect(pexpect.EOF)
assert "Make the window bigger" not in s.log.getvalue()
print(f"F3) a too-small window (80x24): says 'Maximize this window first ...', adds the zoom-out tip when it is still too small, and draws the animation (at least {need_cols}x{need_lines}) once it is big enough: OK")

# ... and typing q skips the QR code (the signed file is already saved)
s = Session(["sign", second, "--psbt", "send.psbt"])
s.c.setwinsize(24, 80)
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.expect(r"Choose 1 or 2 \[1\]: "); s.send()
s.text("Type q and press Enter to skip."); s.expect(r"> "); s.send("q"); s.expect(pexpect.EOF)
assert "Frame 1/" not in s.log.getvalue()
print("F3b) typing q at the size prompt skips the QR code without drawing anything: OK")

# TERM_PROGRAM=Apple_Terminal (exactly what macOS's own Terminal app sets). A photo of the compact code in that app
# showed a thin white line through the black parts at the end of every line of text, and filling in those lines alone
# made the photo readable. So there SeedLock draws the full style (no block characters at all) and explains it in a box.
apple_env = dict(os.environ, TERM_PROGRAM="Apple_Terminal")
NOTE = "This is Apple's built-in Terminal app"
s = sign_session(env=apple_env)
s.expect(r"How do you want to give the signed transaction back to Sparrow\?")
s.expect(r"Choose 1 or 2 \[1\]: "); s.send()
s.expect(r"Frame 1/(\d+)")
total = int(s.match.group(1))
before_frame = s.log.getvalue()
box = before_frame[before_frame.index("\x1b[1m┌") : before_frame.index("┘\x1b[0m") + len("┘\x1b[0m")]
box_lines = [ANSI.sub("", l).rstrip("\r") for l in box.split("\n") if l.strip()]
assert len({len(l) for l in box_lines}) == 1, f"every row of the box must be the same width: {box_lines}"
assert box_lines[0].startswith("┌") and box_lines[0].endswith("┐") and box_lines[-1].startswith("└") and box_lines[-1].endswith("┘")
assert all(l.startswith("│") and l.endswith("│") for l in box_lines[1:-1]), "every inner row must be bordered on both sides"
for wanted in (NOTE, "IMPORTANT NOTE", "Cmd and -", "iTerm2", "--qr-browser"):
    assert wanted in before_frame, wanted
s.expect(f"Frame {total}/{total}"); s.expect(r"Frame 1/\d+ - press Enter")   # every frame drawn at least once
s.send(); s.expect(pexpect.EOF)
out = s.log.getvalue()
frames = []
for chunk in out.split("\x1b[?2026h")[1:]:
    grid = full_grid(chunk.split("\x1b[?2026l")[0])
    assert len(grid) >= 29 and all(len(row) == len(grid) for row in grid), "a square code, one line of text per row"
    t = grid_text(grid)
    assert t.startswith("UR:CRYPTO-PSBT/"), t[:30]
    if t not in frames:
        frames.append(t)
assert len(frames) == total, f"{len(frames)} distinct frames decoded of {total}"
signed_path = re.search(r"The signed PSBT is saved as:\r?\n\s+(\S+)", out).group(1)
if URDecoder is None:
    print(f"F4) Apple Terminal, animated: boxed note; all {total} frames in the full style decoded (reference UR decoder not available: reassembly skipped)")
else:
    dec = URDecoder()
    for f in frames:
        dec.receive_part(f.lower())
    assert dec.is_complete() and cbor2.loads(bytes(dec.result_message().cbor)) == open(signed_path, "rb").read()
    print(f"F4) TERM_PROGRAM=Apple_Terminal, animated QR: the boxed 'IMPORTANT NOTE' (even width, bordered, naming Cmd and -, iTerm2, --qr-browser), "
          f"then all {total} frames in the full style ({len(grid)} lines, no block characters) decoded and reassembled by the Foundation UR library == signed PSBT: OK")

s = sign_session(["--static-qr"], env=apple_env, dims=(140, 250))
s.expect(pexpect.EOF)
static_out = s.log.getvalue()
assert NOTE in static_out and static_out.index("The signed PSBT is saved as") < static_out.index(NOTE)
assert "Your window is" not in static_out, "the test window is big enough: no size prompt expected"
grid = full_grid(static_out[static_out.index(NOTE):])
signed_path = re.search(r"The signed PSBT is saved as:\r?\n\s+(\S+)", static_out).group(1)
assert bytes(PSBT.from_string(grid_text(grid)).serialize()) == open(signed_path, "rb").read()
print(f"F4b) TERM_PROGRAM=Apple_Terminal, static QR: the note, then the full style ({len(grid)} lines), decoded to the base64 PSBT == the signed file: OK")

s = Session(["qr", second], env=apple_env, dims=(140, 250))
s.expect(pexpect.EOF)
qr_out = s.log.getvalue()
assert NOTE in qr_out and "Your window is" not in qr_out, qr_out[-400:]
grid = full_grid(qr_out[qr_out.index(NOTE):])
backup_line = [l for l in open(os.path.join(work, "seedlocker backups", second + ".seedlock")).read().splitlines() if l.startswith("SEEDLOCK1:")][0]
assert grid_text(grid) == backup_line
print(f"F4c) TERM_PROGRAM=Apple_Terminal, backup QR (seedlock qr): the note, then the full style ({len(grid)} lines), decoded == the backup text: OK")

# ---------------------------------------------------------------- H) is this computer online? (said before the menu)
s = Session(env=dict(os.environ, SEEDLOCK_TEST_NET_DIR=os.path.join(NET, "online")))
s.text("WARNING: THIS COMPUTER IS ONLINE")
s.text("It is connected to a network (through wlan0).")
s.text("unplug any network cable.")
s.expect(r"\n> ")
box = s.log.getvalue()
box = box[box.index("\x1b[1m┌"): box.index("┘\x1b[0m") + len("┘\x1b[0m")]
box_lines = [ANSI.sub("", l).rstrip("\r") for l in box.split("\n") if l.strip()]
assert len({len(l) for l in box_lines}) == 1 and all(l[0] in "┌│└" and l[-1] in "┐│┘" for l in box_lines), box_lines
s.send("x"); s.text("Please choose one of the numbers above.")
s.text("(Reminder: this computer is still ONLINE, through wlan0.)")        # each time the menu comes back
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
assert s.log.getvalue().count("WARNING: THIS COMPUTER IS ONLINE") == 1, "the full warning once, then a reminder"
s = Session()                                                             # offline (these tests' stand-in)
s.text("This computer is offline: no network connection was found.")
s.expect(r"\n> "); s.send("x"); s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
assert s.log.getvalue().count("This computer is offline") == 1 and "ONLINE" not in s.log.getvalue()
s = Session(env=dict(os.environ, SEEDLOCK_TEST_NET_DIR="/nonexistent/seedlock"))
s.text("SeedLock cannot tell whether this computer is online (the routing table could not be read).")
s.text("Make sure Wi-Fi is off and no network cable is plugged in.")
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
print("H) before the menu: an online computer gets a boxed warning naming its connection, then a reminder each time the menu comes back; "
      "offline is said once; a routing table that cannot be read is said plainly: OK")

# ---------------------------------------------------------------- H2) in a virtual machine: its network adapter always looks connected
import shutil as _sh
for _state in ("online", "offline"):
    _vm = os.path.join(NET, _state + "-vm")
    _sh.copytree(os.path.join(NET, _state), _vm)
    open(os.path.join(_vm, "sys_vendor"), "w").write("QEMU\n")                      # the firmware description a QEMU/KVM guest shows
    open(os.path.join(_vm, "product_name"), "w").write("Standard PC (Q35 + ICH9, 2009)\n")
s = Session(env=dict(os.environ, SEEDLOCK_TEST_NET_DIR=os.path.join(NET, "online-vm")))
s.text("WARNING: THIS VIRTUAL MACHINE HAS A NETWORK CONNECTION")
s.expect(r"\n> ")
vm_text = ANSI.sub("", s.log.getvalue())
flat = " ".join(l.strip(" │\r") for l in vm_text.splitlines())
for wanted in ("SeedLock is running in a virtual machine (QEMU/KVM) whose network adapter is switched on (through wlan0).",
               "even while the real computer is offline", "SeedLock cannot see the real computer's connection",
               "turn off this virtual machine's network adapter", "then SeedLock shows it as offline"):
    assert wanted in " ".join(flat.split()), (wanted, flat[-900:])
assert "THIS COMPUTER IS ONLINE" not in vm_text
s.send("x"); s.text("Please choose one of the numbers above.")
s.text("(Reminder: this virtual machine's network adapter is still on, through wlan0.)")
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
s = Session(env=dict(os.environ, SEEDLOCK_TEST_NET_DIR=os.path.join(NET, "offline-vm")))
s.text("This computer is offline: no network connection was found.")       # adapter off: offline, as on any computer
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
assert "VIRTUAL MACHINE" not in s.log.getvalue()
print("H2) in a virtual machine (a QEMU firmware description): the box says it is a virtual machine whose adapter always looks "
      "connected, that the real computer's connection cannot be seen from inside, and to turn the adapter off; then a reminder; "
      "with the adapter off it is simply offline: OK")

# ---------------------------------------------------------------- I) Ctrl+C at a hidden prompt puts the terminal back
import signal, termios
s = Session(["encrypt"])
s.text("Enter your mnemonic words separated by spaces (hidden as you type):")
s.expect(r"\n> ")
time.sleep(0.3)                                       # the terminal is now switched to hidden typing
s.c.send("zoo zoo")
time.sleep(0.3)
s.c.send("\x03")                                      # Ctrl+C
s.expect(pexpect.EOF)
modes = termios.tcgetattr(s.c.child_fd)[3]            # the terminal's modes, as the next program finds them
s.c.close()
assert s.c.signalstatus == signal.SIGINT, (s.c.exitstatus, s.c.signalstatus)
assert modes & termios.ECHO and modes & termios.ICANON and modes & termios.ISIG, "the terminal was left showing nothing that is typed"
assert "zoo zoo" not in s.log.getvalue()
print("I) Ctrl+C at a hidden prompt ends SeedLock as Ctrl+C always does, and the terminal is put back first (it shows what is typed again): OK")

# ---------------------------------------------------------------- global checks over every screen
log = ALL_LOGS.getvalue()
assert "--force" not in log and "overwrite it" not in log, "the tool must never suggest overwriting"
for secret in (PW2, M2, "Another-Long-Password-9", "3" * 30, "6" * 45, "5" * 25, "bad/name-x"):
    assert secret not in log, f"a secret was echoed to the screen: {secret[:12]}..."
assert log.count(pw1) == 1, "the generated password must be shown exactly once"
print("G) no '--force' anywhere; typed passwords, words and dice rolls were never echoed; generated password shown once: OK")
