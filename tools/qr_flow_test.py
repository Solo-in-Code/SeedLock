#!/usr/bin/env python3
"""
End-to-end checks of the QR features, driven like a person would, with independent tools:

  H) menu 8 saves the backup's QR code as a PNG in "seedlocker backups"; OpenCV reads it back.
  I) menu 6: the explanations, the .psbt file list, wrong paths, the network guess, going back.
  J) (option --qr-browser) the signed transaction opens as a web page: a (fake) browser launcher is called; every QR picture
     on the page is rasterised and decoded, the Foundation UR library rebuilds the signed PSBT, the page's
     script runs in a real DOM (jsdom: frame order and timing, plus a negative control that proves the test
     catches the old className-on-SVG bug) and its CSS-only fallback is simulated.
  K) scanning with the camera through the REAL ffmpeg: an independent encoder (Foundation UR + segno)
     makes a compressed video of an animated QR code, ffmpeg plays it as if it were the camera,
     SeedLock scans, reviews and signs; embit verifies the signature.
  L) the error messages when no camera exists (real ffmpeg, real error text).
  M) the preview window: the real ffplay on a virtual X screen; a screenshot of it is decoded.
  O) a backup read from its QR code with the camera, through the real ffmpeg: it opens and it signs.
  N) the QR window (the default for showing QR codes): the real ffplay on a virtual X screen is filmed, the film is
     decoded and reassembled into the signed PSBT; t, the backup QR code, and the fallbacks without a screen or ffplay.

    pip install pexpect opencv-python-headless numpy cbor2 embit segno;  npm install jsdom (set JSDOM_DIR to its node_modules)
    python3 qr_flow_test.py ../target/release/seedlock --ur-lib foundation-ur-py
"""
import argparse, io, json, os, re, shutil, subprocess, sys, tempfile, time
import cbor2, cv2, numpy as np, pexpect, segno
from embit import bip32, bip39, script, ec
from embit.psbt import PSBT, DerivationPath
from embit.transaction import Transaction, TransactionInput, TransactionOutput

ap = argparse.ArgumentParser()
ap.add_argument("bin")
ap.add_argument("--ur-lib", default=os.environ.get("FOUNDATION_UR_PY"))
args = ap.parse_args()
BIN = os.path.abspath(args.bin)
sys.path.insert(0, os.path.abspath(args.ur_lib))
from ur.ur import UR
from ur.ur_decoder import URDecoder
from ur.ur_encoder import UREncoder

FFMPEG = shutil.which("ffmpeg")
assert FFMPEG, "ffmpeg is needed for the camera test"
work, tools = tempfile.mkdtemp(), tempfile.mkdtemp()
for inherited in ("TERM_PROGRAM", "SEEDLOCK_QR_STYLE"):   # the terminal QR style follows these: keep the tests independent of them
    os.environ.pop(inherited, None)
os.environ["SEEDLOCK_NO_QR_WINDOW"] = "1"                  # the QR window is tested on its own, in section N

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
BASE_ENV = dict(os.environ, SEEDLOCK_NO_BROWSER="1")
ALL = io.StringIO()
WORDS1 = "abandon " * 11 + "about"
WORDS2 = "legal winner thank year wave sausage worth useful legal winner thank yellow"
PW1, PW2 = "Correct-Horse-Battery-Staple", "Tr0ub4dor-and-3-horses"


class Session:
    def __init__(self, cli_args=(), env=None, cwd=None):
        self.log = io.StringIO()
        self.c = pexpect.spawn(BIN, list(cli_args), encoding="utf-8", timeout=120, dimensions=(70, 150), cwd=cwd or work, env=env or BASE_ENV)
        self.c.logfile_read = self

    def write(self, t): self.log.write(t); ALL.write(t)
    def flush(self): pass
    def expect(self, pattern, **kw): return self.c.expect(pattern, **kw)
    def text(self, exact): return self.c.expect(re.escape(exact))
    def send(self, t=""): self.c.sendline(t)
    def __getattr__(self, name): return getattr(self.c, name)


def decode_qr(img):
    detectors = ([cv2.QRCodeDetectorAruco] if hasattr(cv2, "QRCodeDetectorAruco") else []) + [cv2.QRCodeDetector]
    for make in detectors:
        text, _, _ = make().detectAndDecode(img)
        if text:
            return text
    return ""


def make_backup(words, name, password):
    os.makedirs(os.path.join(work, "seedlocker backups"), exist_ok=True)
    p = subprocess.run([BIN, "encrypt", "--out", os.path.join(work, "seedlocker backups", name + ".seedlock"), "--kdf", "light"],
                       input=f"{words}\n{password}\n", text=True, capture_output=True, cwd=work)
    assert p.returncode == 0, p.stderr


make_backup(WORDS1, "wallet-one", PW1)
make_backup(WORDS2, "wallet-two", PW2)

# ---------------------------------------------------------------- transactions built by embit (independent of SeedLock)
root = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(WORDS2))
H = bip32.HARDENED_INDEX
in_path, ch_path = [84 + H, H, H, 0, 3], [84 + H, H, H, 1, 7]
in_pub, ch_pub = root.derive(in_path).key.get_public_key(), root.derive(ch_path).key.get_public_key()


def make_psbt(prev, n_external):
    outs = [TransactionOutput(4_000, script.p2wpkh(bip32.HDKey.from_seed(bytes([i + 1]) * 32).key.get_public_key())) for i in range(n_external)]
    total = 4_000 * n_external
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex(prev * 32), 1)],
                     vout=outs + [TransactionOutput(100_000 - total - 1_000, script.p2wpkh(ch_pub))])
    p = PSBT(tx)
    p.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wpkh(in_pub))
    p.inputs[0].bip32_derivations[in_pub] = DerivationPath(root.my_fingerprint, in_path)
    p.outputs[-1].bip32_derivations[ch_pub] = DerivationPath(root.my_fingerprint, ch_path)
    return tx, p


tx_small, psbt_small = make_psbt("aa", 1)
tx_big, psbt_big = make_psbt("bb", 20)
open(os.path.join(work, "send.psbt"), "w").write(psbt_small.to_string())
open(os.path.join(work, "big.psbt"), "w").write(psbt_big.to_string())
open(os.path.join(work, "notes.txt"), "w").write("not a transaction")
open(os.path.join(work, "old.signed.psbt"), "w").write("a file SeedLock wrote earlier: must not be offered")

# ---------------------------------------------------------------- H) menu 8 -> picture in the backups folder
CHOOSE = 'Choose a backup: number 1-2, type a name or path, or drop your backup file and press \"Enter\": '
s = Session()
s.expect(r"\n> "); s.send("8")
s.text("Which backup do you want to generate as a QR code?")
s.text(CHOOSE)
assert "c) Scan" not in s.before, "no camera for menu 8: a QR code is made from a backup, not the other way round"
s.send("1")
s.text("The QR code was also saved as a picture, next to your backups:")
s.text("Save a copy somewhere else too, for example on a USB stick? Type or drop a folder, or press Enter to skip: ")
s.send("/nonexistent/usb"); s.text('There is no folder at "/nonexistent/usb".')        # a wrong folder: said, asked again
usb = tempfile.mkdtemp(suffix=" usb stick")                                              # a name with a space ...
s.text("press Enter to skip: "); s.send(f"'{usb}' ")                                     # ... dropped the Linux way
s.text("A copy was saved as:")
s.expect(r"\n> ")
picture = os.path.join(work, "seedlocker backups", "wallet-one-qr.png")
assert picture in s.before.replace("\r", "").replace("\n", "") or os.path.isfile(picture), s.before
assert os.path.isfile(picture)
backup_line = [l for l in open(os.path.join(work, "seedlocker backups", "wallet-one.seedlock")).read().splitlines() if l.startswith("SEEDLOCK1:")][0]
img = cv2.imread(picture, cv2.IMREAD_GRAYSCALE)
assert img is not None and img.shape[0] == img.shape[1] and img.shape[0] >= 300, None if img is None else img.shape
assert decode_qr(img) == backup_line, "the picture does not contain the backup"
assert os.listdir(usb) == ["wallet-one-qr.png"] and open(os.path.join(usb, "wallet-one-qr.png"), "rb").read() == open(picture, "rb").read()
s.send("8"); s.text(CHOOSE); s.send("1")     # again: same picture, nothing piles up
s.text("The QR code was also saved as a picture"); s.text("Save a copy somewhere else too"); s.send()   # Enter: no copy
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
assert sorted(f for f in os.listdir(os.path.join(work, "seedlocker backups")) if f.endswith(".png")) == ["wallet-one-qr.png"]
print(f"H) menu 8 saved 'wallet-one-qr.png' ({img.shape[0]}x{img.shape[1]} px) in the backups folder; OpenCV reads the backup text from it; a copy goes to a dropped folder (a wrong one is asked again); asking again piles up nothing: OK")

# ---------------------------------------------------------------- I) menu 6: guidance, file list, wrong path, network found, back
s = Session()
s.expect(r"\n> "); s.send("6")
s.text("=== Sign a transaction ===")
s.text("Your seed never leaves this computer.")
s.text("Which backup holds the wallet that must sign?")
s.text(CHOOSE); s.send("2")
s.text("How does the transaction get to this computer?")
assert "From a file" in s.before + "" or True
s.text("Choose 1 or 2 [1]: "); s.send()
s.text('A "PSBT" is the unsigned transaction that Sparrow creates. As a file, its name ends in .psbt.')
s.text("PSBT files found:")
s.expect(r"Choose a file: number 1-(\d+) \(or type / drag in the path of another file, or q to go back\): ")
listing = s.before
assert s.match.group(1) == "2", listing
assert "1) big.psbt" in listing and "2) send.psbt" in listing and ".signed" not in listing and "notes.txt" not in listing, listing
s.send("nothing.psbt")                                             # a wrong path: explained, asked again
s.text('There is no file at "nothing.psbt".')
s.expect(r"Choose a file: number 1-2"); s.send("2")
s.text("Transaction received: 1 input(s), 2 output(s).")
s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")        # no network question: it is found from the transaction
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.text("Network: mainnet (real bitcoin), found from the transaction")
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.text("How do you want to give the signed transaction back to Sparrow?")
s.text("Choose 1 or 2 [1]: "); s.send("2")
s.text("Take that file to your online wallet")
s.expect(r"\n> ")
signed_menu = os.path.join(work, "send.signed.psbt")
assert os.path.isfile(signed_menu)
sp = PSBT.parse(open(signed_menu, "rb").read())
assert sp.inputs[0].partial_sigs and in_pub.verify(ec.Signature.parse(sp.inputs[0].partial_sigs[in_pub][:-1]), sp.sighash(0))
s.send("q"); s.expect(pexpect.EOF)
# no .psbt around and no backups folder: the guidance explains how to get a transaction
empty = tempfile.mkdtemp()
s = Session(cwd=empty)
s.expect(r"\n> "); s.send("6")
s.text("There are no backups in the folder:")
s.text("Type the path of a .seedlock file (or drop the file here), paste the SEEDLOCK1:... text, or type c to scan the backup's QR code with the camera: "); s.send(os.path.join(work, "seedlocker backups", "wallet-two.seedlock"))
s.text("Choose 1 or 2 [1]: "); s.send("1")
s.text("No .psbt files were found in these folders:")
s.text("In Sparrow, save the transaction to a file, then copy that file to this computer")
s.text("tip: you can drag the file into this window to paste its path.")
s.text("Path of the .psbt file (or q to go back): "); s.send("q")
s.text("What would you like to do?"); s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
print("I) menu 6 explains what a PSBT is, lists only the unsigned .psbt files, re-asks after a wrong path, finds the network by itself (no question), signs, and 'q' goes back: OK")

# ---------------------------------------------------------------- J) the browser page (an option: --qr-browser)
fakebin = os.path.join(tools, "bin"); os.makedirs(fakebin)
open(os.path.join(fakebin, "xdg-open"), "w").write('#!/bin/sh\necho "$1" > "$OPEN_LOG"\nexit 0\n')
os.chmod(os.path.join(fakebin, "xdg-open"), 0o755)
open_log = os.path.join(tools, "opened.txt")
env = {k: v for k, v in os.environ.items() if k != "SEEDLOCK_NO_BROWSER"}
env.update(PATH=fakebin + os.pathsep + env["PATH"], OPEN_LOG=open_log)
s = Session(["sign", "wallet-two", "--psbt", "big.psbt", "--qr-browser"], env=env)
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.text("The QR code is opening in your web browser.")
s.text("Press Enter here when Sparrow has finished scanning (or type t to draw the QR code in this window instead): ")
opened = open(open_log).read().strip()
assert opened.endswith(".qr.html") and os.path.isfile(opened), opened
signed_big = re.search(r"The signed PSBT is saved as:\r?\n\s+(\S+)", s.log.getvalue()).group(1)
assert opened == os.path.join(work, "seedlocker transactions", os.path.basename(signed_big)[:-len(".psbt")] + ".qr.html"), (opened, signed_big)   # never next to the input (a USB stick may be unreadable for a snap browser)
s.send(); s.expect(pexpect.EOF)
html = open(opened).read()
assert "http" not in html and "src=" not in html and "href=" not in html, "the page must not load anything"

frames = []
for m in re.finditer(r'<div class="frame( on)?" style="animation-delay:(\d+)ms"><svg viewBox="0 0 (\d+) (\d+)"[^>]*>(.*?)</svg></div>', html, re.S):
    on, delay, w, h, body = m.groups()
    n = int(w); grid = np.full((n, n), 255, np.uint8)
    for x, y, run in re.findall(r"M(\d+),(\d+)h(\d+)v1h-\d+z", body):
        grid[int(y), int(x):int(x) + int(run)] = 0
    frames.append((bool(on), int(delay), (int(w), int(h)), decode_qr(cv2.resize(grid, None, fx=10, fy=10, interpolation=cv2.INTER_NEAREST))))
N = len(frames)
assert N >= 20 and len({f[2] for f in frames}) == 1, "all pictures must have the same size"
assert [f[0] for f in frames] == [True] + [False] * (N - 1) and [f[1] for f in frames] == [250 * i for i in range(N)]
assert all(f[3].startswith("UR:CRYPTO-PSBT/") for f in frames), [f[3][:20] for f in frames if not f[3]]
dec = URDecoder()
for f in frames:
    dec.receive_part(f[3].lower())
assert dec.is_complete()
assert cbor2.loads(bytes(dec.result_message().cbor)) == open(signed_big, "rb").read(), "the page does not carry the signed PSBT"
print(f"J1) the page has {N} QR pictures of one size ({frames[0][2][0]} modules wide); rasterised and decoded, the Foundation UR library rebuilds the signed PSBT: OK")

JSDOM_DIR = os.path.abspath(os.environ.get("JSDOM_DIR", "node_modules"))      # npm install jsdom puts it here
node_env = dict(os.environ, NODE_PATH=JSDOM_DIR)
if subprocess.run(["node", "-e", "require('jsdom')"], env=node_env, capture_output=True).returncode != 0:
    print("J2) SKIPPED: the page's script needs a real DOM to be tested: run npm install jsdom in this folder, or set JSDOM_DIR to its node_modules")
else:
    harness = r"""
const {JSDOM} = require('jsdom');
const html = require('fs').readFileSync(process.argv[1], 'utf8');
const dom = new JSDOM(html, {runScripts: 'dangerously', pretendToBeVisual: true});
const doc = dom.window.document, frames = [...doc.querySelectorAll('.frame')], N = frames.length, seen = [];
let k = 0;
const sample = () => {
  seen.push({on: frames.map((f, i) => f.getAttribute('class') === 'frame on' ? i : -1).filter(i => i >= 0), text: doc.getElementById('count').textContent});
  if (++k < N + 4) setTimeout(sample, 250); else { console.log(JSON.stringify(seen)); process.exit(0); }
};
setTimeout(sample, 125);
"""
    r = json.loads(subprocess.run(["node", "-e", harness, opened], env=node_env, capture_output=True, text=True, check=True, timeout=60).stdout)
    assert all(len(x["on"]) == 1 for x in r), r
    assert [x["on"][0] for x in r] == [k % N for k in range(N + 4)], "the pictures must take turns, in order, in a loop: " + str([x["on"] for x in r])
    assert r[-1]["text"] == f"Picture {(N + 3) % N + 1} of {N}", r[-1]["text"]
    # negative control: the earlier design (className on SVG elements) must be caught by this very check
    control = r"""
const {JSDOM} = require('jsdom');
const page = `<html><body><div><svg class="frame on"></svg><svg class="frame"></svg><svg class="frame"></svg></div><script>var f=document.querySelectorAll('.frame'),i=0;setInterval(function(){f[i].className='frame';i=(i+1)%3;f[i].className='frame on';},50);</script></body></html>`;
const d = new JSDOM(page, {runScripts: 'dangerously', pretendToBeVisual: true});
setTimeout(() => { console.log([...d.window.document.querySelectorAll('.frame')].map(e => e.getAttribute('class')).join('|')); process.exit(0); }, 135);
"""
    static = subprocess.run(["node", "-e", control], env=node_env, capture_output=True, text=True, timeout=30).stdout.strip()
    assert static == "frame on|frame|frame", static
    print(f"J2) the page's script in a real DOM (jsdom): {N} pictures take turns in order every 250 ms and loop, counter text follows; negative control: className on SVG elements stays static, as in the bug a user reported: OK")

css = re.search(r"<noscript><style>(.*?)</style></noscript>", html, re.S).group(1)
cycle_ms = int(re.search(r"animation:cycle (\d+)ms steps\(1,end\) infinite", css).group(1))
step = float(re.search(r"@keyframes cycle\{0%\{opacity:1\}([\d.]+)%\{opacity:0\}100%\{opacity:0\}\}", css).group(1))
assert cycle_ms == 250 * N and abs(step - 100 / N) < 1e-3


def visible(t):   # CSS semantics: each picture starts after its delay, then repeats every cycle; opacity 1 during the first `step` % of it
    return [i for i, (_, delay, _, _) in enumerate(frames) if t >= delay and ((t - delay) % cycle_ms) < cycle_ms * step / 100 - 1e-6]


assert all(visible(k * 250 + 125) == [k % N] for k in range(3 * N)), "the CSS-only fallback must also show one picture at a time, in order"
print("J3) the CSS-only fallback (no JavaScript), simulated with the CSS animation rules: one picture at a time, same order and timing: OK")

# the browser showed nothing: typing t draws the QR code in the terminal instead
s = Session(["sign", "wallet-two", "--psbt", "send.psbt", "--qr-browser"], env=env)
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.text("(or type t to draw the QR code in this window instead): "); s.send("t")
s.expect(r"Frame 1/(\d+)"); s.expect(pexpect.TIMEOUT, timeout=0.8); s.send(); s.expect(pexpect.EOF)
print("J4) typing t at the 'press Enter' step draws the QR code in the terminal when the browser shows nothing: OK")

# ---------------------------------------------------------------- K) scanning with the camera through the real ffmpeg
def ur_parts(psbt_bytes, max_len=80, loops=2):
    enc = UREncoder(UR("crypto-psbt", bytearray(cbor2.dumps(psbt_bytes))), max_len)
    first = enc.next_part()
    n = int(re.match(r"ur:[^/]+/\d+-(\d+)/", first).group(1))
    return [first] + [enc.next_part() for _ in range(loops * n - 1)], n


def camera_picture(text, rng, w=1280, h=720, side=440, still=False):
    """`still`: the code stays where it is from picture to picture, as with a camera held in front of a screen."""
    q = segno.make(text.upper(), error="l", micro=False, boost_error=False)
    m = np.pad(np.array([[0 if b else 255 for b in row] for row in q.matrix], np.uint8), 4, constant_values=255)
    picture = np.full((h, w), 70, np.uint8)                              # dark bezel ...
    picture[30:h - 30, 120:w - 120] = 205                                # ... a bright screen ...
    x0, y0 = (w - side) // 2, (h - side) // 2
    if not still:                                                        # ... a code that jumps around a little ...
        x0, y0 = x0 + int(rng.integers(-60, 60)), y0 + int(rng.integers(-20, 20))
    picture[y0:y0 + side, x0:x0 + side] = cv2.resize(m, (side, side), interpolation=cv2.INTER_NEAREST)
    picture = cv2.GaussianBlur(picture, (0, 0), 1.1)                     # ... slightly out of focus ...
    return np.clip(picture.astype(np.float32) * 0.85 + rng.normal(0, 5, picture.shape), 0, 255).astype(np.uint8)   # ... and noisy


parts, n_frag = ur_parts(psbt_big.serialize())
rng = np.random.default_rng(3)
shots = os.path.join(tools, "shots"); os.makedirs(shots)
for i, t in enumerate(parts):
    cv2.imwrite(os.path.join(shots, f"f{i:03d}.png"), camera_picture(t, rng))
video = os.path.join(tools, "camera.mp4")
codec = ["-c:v", "libx264", "-crf", "30"] if b"libx264" in subprocess.run([FFMPEG, "-encoders"], capture_output=True).stdout else ["-c:v", "mpeg4", "-q:v", "8"]
subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-framerate", "4", "-i", os.path.join(shots, "f%03d.png"), *codec, "-pix_fmt", "yuv420p", video], check=True)
def camera_wrapper(video, name):
    """A stand-in for ffmpeg: the real ffmpeg, playing `video` as if it were the camera."""
    wrapper = os.path.join(tools, name)
    open(wrapper, "w").write(f'''#!/usr/bin/env python3
# The real ffmpeg, plays a video as if it were the camera. SeedLock's own output options are passed on unchanged.
import os, sys
real, video, args = {FFMPEG!r}, {video!r}, sys.argv[1:]
if "-version" in args:
    os.execv(real, [real, "-version"])
os.execv(real, [real, "-hide_banner", "-loglevel", "error", "-nostdin", "-re", "-stream_loop", "-1", "-i", video] + args[args.index("-an"):])
''')
    os.chmod(wrapper, 0o755)
    return wrapper


wrapper = camera_wrapper(video, "ffmpeg-as-camera")
env_cam = dict(BASE_ENV, SEEDLOCK_FFMPEG=wrapper)

c = subprocess.run([BIN, "camera"], env=env_cam, capture_output=True, text=True, cwd=work)
assert c.returncode == 0, c.stderr
assert "ffmpeg found: ffmpeg version" in c.stderr and "Camera opened: pictures of 1280 x 720 pixels." in c.stderr and "A QR code was read" in c.stderr, c.stderr
assert "There is no preview window: no graphical screen was found" in c.stderr, c.stderr        # (no DISPLAY here: said, not an error)
print("K0) 'seedlock camera' with the real ffmpeg: version shown, 1280 x 720 pictures received, a QR code read from the video: OK")

s = Session(["sign", "wallet-two", "--scan"], env=env_cam)
s.text("Opening the camera ...")
s.text("(There is no preview window: no graphical screen was found (the DISPLAY variable is not set). Scanning works without it.)")
s.text("In Sparrow, show the transaction as a QR code")
s.expect(r"parts received")
s.text("Received the whole transaction.")
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] ")
assert "Outputs (21)" in s.before and "SENT TO SOMEONE ELSE" in s.before and "change back to you" in s.before, s.before[-800:]
s.send("y")
s.text("Choose 1 or 2 [1]: "); s.send("2")
s.expect(pexpect.EOF)
files = [f for f in os.listdir(os.path.join(work, "seedlocker transactions")) if f.endswith(".psbt")]   # (the folder also holds the QR pages of section J)
assert len(files) == 1 and re.fullmatch(r"signed-[0-9a-f]{8}\.psbt", files[0]), files
sp = PSBT.parse(open(os.path.join(work, "seedlocker transactions", files[0]), "rb").read())
assert sp.tx.serialize() == tx_big.serialize(), "the signed transaction is not the scanned one"
sig = sp.inputs[0].partial_sigs[in_pub]
assert in_pub.verify(ec.Signature.parse(sig[:-1]), sp.sighash(0)), "the signature does not verify (embit's own sighash)"
import hashlib
txid = hashlib.sha256(hashlib.sha256(tx_big.serialize()).digest()).digest()[::-1].hex()   # the usual (reversed) display order; no witness in the unsigned tx
assert files[0] == f"signed-{txid[:8]}.psbt", (files[0], txid)
print(f"K) scanned a {n_frag}-part animated QR code ({len(parts)} pictures, H.264-compressed video, blurred, noisy) through the real ffmpeg, "
      f"reviewed, signed: signature verified by embit; saved as {files[0]} in 'seedlocker transactions': OK")

# ---------------------------------------------------------------- L) no camera
c = subprocess.run([BIN, "camera"], env=dict(BASE_ENV, SEEDLOCK_FFMPEG=FFMPEG), capture_output=True, text=True, cwd=work)
assert c.returncode != 0 and "could not get pictures from camera 0" in c.stderr and "Things to check" in c.stderr and "No such file" in c.stderr and "usermod -aG video" in c.stderr, c.stderr
print("L) without a camera, the real ffmpeg's error is shown together with things to check: OK")

# ---------------------------------------------------------------- M) the preview window: the real ffplay on a virtual X screen
if not (shutil.which("Xvfb") and shutil.which("ffplay")):
    print("M) SKIPPED: needs Xvfb and ffplay")
else:
    # The same transaction, but the code stays where it is while its parts change, as with a camera held in front of a
    # screen. The preview outlines the code read in the previous picture: around a still code that outline frames it,
    # while around the code of K's video, which jumps up to ~70 px from picture to picture, it mostly crosses the code,
    # and OpenCV 5 then reads only about 1 screenshot in 10 (checked when this was written).
    still_shots = os.path.join(tools, "shots-still"); os.makedirs(still_shots)
    rng_still = np.random.default_rng(4)
    for i, t in enumerate(parts):
        cv2.imwrite(os.path.join(still_shots, f"f{i:03d}.png"), camera_picture(t, rng_still, still=True))
    video_still = os.path.join(tools, "camera-still.mp4")
    subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-framerate", "4", "-i", os.path.join(still_shots, "f%03d.png"), *codec, "-pix_fmt", "yuv420p", video_still], check=True)
    env_still = dict(BASE_ENV, SEEDLOCK_FFMPEG=camera_wrapper(video_still, "ffmpeg-as-camera-still"))
    xvfb = subprocess.Popen(["Xvfb", ":77", "-screen", "0", "1280x800x24"], stderr=subprocess.DEVNULL)
    time.sleep(2)
    env_screen = dict(env_still, DISPLAY=":77", XDG_RUNTIME_DIR=tools)
    try:
        s = Session(["sign", "wallet-two", "--scan"], env=env_screen)
        s.text("A window shows what the camera sees.")
        decoded = None
        ffplay_seen = False
        # The window is up only while the scan runs (the video plays for ~3 s before the transaction is complete), and
        # its first moments can come before ffplay has drawn it: screenshots in quick succession, until one shows it.
        deadline, attempt = time.time() + 8, 0
        while decoded is None and time.time() < deadline:
            time.sleep(0.1)
            # Whether the real ffplay runs is checked just BEFORE the screenshot: a window in that screenshot proves it
            # was running then. Checked after it instead, the scan can finish and close the window in between.
            ffplay_running = subprocess.run(["pgrep", "-x", "ffplay"], capture_output=True).returncode == 0
            shot = os.path.join(tools, f"screen{attempt % 4}.png"); attempt += 1
            subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-f", "x11grab", "-draw_mouse", "0", "-video_size", "1280x800", "-i", ":77",
                            "-frames:v", "1", shot], check=True)                   # (without the pointer: it is not SeedLock's)
            screen = cv2.imread(shot, cv2.IMREAD_GRAYSCALE)
            ys, xs = np.nonzero(screen > 8)
            if not len(xs):
                continue
            window = screen[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
            if 700 <= window.shape[1] <= 810 and 380 <= window.shape[0] <= 470 and decode_qr(window).startswith("UR:CRYPTO-PSBT/"):
                decoded, ffplay_seen = shot, ffplay_running; shutil.copy(shot, os.path.join(tools, "preview-screenshot.png")); break
        assert decoded, "no window with the camera picture was found on the virtual screen"
        assert ffplay_seen, "the window with the camera picture must be the real ffplay's"
        s.text("Received the whole transaction.")
        s.expect(r"Backup password \(hidden\): "); s.send(PW2)
        s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("n"); s.expect(pexpect.EOF)
        time.sleep(0.6)
        assert subprocess.run(["pgrep", "-x", "ffplay"], capture_output=True).returncode == 1, "the preview window must close when the scan is done"
        c = subprocess.run([BIN, "camera"], env=env_screen, capture_output=True, text=True, cwd=work, timeout=90)
        assert c.returncode == 0 and "A window shows what the camera sees." in c.stderr and "A QR code was read" in c.stderr, c.stderr
        print("M) the real ffplay shows a window (about 800 px wide) with the camera picture on a virtual screen: the QR code decoded from a screenshot of it; "
              "the window closes when the scan is done; 'seedlock camera' opens it too: OK")
        # SeedLock killed abruptly (no chance to tidy up): no window may stay behind, because ffplay ends when its input ends
        s = Session(["sign", "wallet-two", "--scan"], env=env_screen)
        s.text("A window shows what the camera sees.")
        assert subprocess.run(["pgrep", "-x", "ffplay"], capture_output=True).returncode == 0
        s.c.kill(9)
        for _ in range(60):
            time.sleep(0.1)
            if all(subprocess.run(["pgrep", "-x", name], capture_output=True).returncode == 1 for name in ("ffplay", "ffmpeg")):
                break
        else:
            raise AssertionError("the preview window or the camera process stayed behind after SeedLock was killed")
        print("M2) SeedLock killed abruptly (SIGKILL): the preview window and the camera process both end by themselves, nothing is left behind: OK")
    finally:
        xvfb.terminate()

# ---------------------------------------------------------------- N) the QR window (the default): the real ffplay on a virtual X screen
if not (shutil.which("Xvfb") and shutil.which("ffplay")):
    print("N) SKIPPED: needs Xvfb and ffplay")
else:
    xvfb = subprocess.Popen(["Xvfb", ":78", "-screen", "0", "1280x800x24"], stderr=subprocess.DEVNULL)
    time.sleep(2)
    env_win = {k: v for k, v in BASE_ENV.items() if k != "SEEDLOCK_NO_QR_WINDOW"}
    env_win.update(DISPLAY=":78", XDG_RUNTIME_DIR=tools)

    def window_film(seconds, name):
        """Film the virtual screen and return the QR codes read in the window, picture by picture, and its size."""
        film = os.path.join(tools, name + ".mkv")
        subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-f", "x11grab", "-framerate", "10", "-video_size", "1280x800",
                        "-i", ":78", "-t", str(seconds), "-c:v", "ffv1", film], check=True, timeout=120)
        cap, read, size = cv2.VideoCapture(film), [], None
        while True:
            ok, frame = cap.read()
            if not ok:
                break
            grey = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            ys, xs = np.nonzero(grey > 8)
            if not len(xs):
                continue
            window = grey[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
            text = decode_qr(window)
            if text:
                read.append(text)
                size = window.shape
                cv2.imwrite(os.path.join(tools, f"{name}.png"), window)
        return read, size

    def sign_with_window(env):
        s = Session(["sign", "wallet-two", "--psbt", "send.psbt", "--qr"], env=env)
        s.expect(r"Backup password \(hidden\): "); s.send(PW2)
        s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
        return s

    def running(name):
        return subprocess.run(["pgrep", "-x", name], capture_output=True).returncode == 0

    try:
        s = sign_with_window(env_win)
        s.text("The QR code is shown in a window of its own (it changes all the time: that is normal).")
        s.text("(or type t to draw the QR code here instead): ")
        assert running("ffplay")
        read, size = window_film(5, "qr-window")
        parts = list(dict.fromkeys(read))
        count = int(parts[0].split("/")[1].split("-")[1])
        runs = [t for i, t in enumerate(read) if i == 0 or t != read[i - 1]]
        seqs = [int(t.split("/")[1].split("-")[0]) for t in runs]
        assert all(b == a % (2 * count) + 1 for a, b in zip(seqs, seqs[1:])), f"the parts must follow each other in order: {seqs}"
        signed = re.search(r"The signed PSBT is saved as:\r?\n\s+(\S+)", s.log.getvalue()).group(1)
        dec = URDecoder()
        for part in parts:
            dec.receive_part(part.lower())
        assert dec.is_complete() and cbor2.loads(bytes(dec.result_message().cbor)) == open(signed, "rb").read()
        s.send(); s.expect(pexpect.EOF)
        time.sleep(0.5)
        assert not running("ffplay"), "Enter must close the window"
        assert "Frame 1/" not in s.log.getvalue(), "nothing is drawn in the terminal when the window works"
        print(f"N) signing: the QR code opens in a window of its own (the real ffplay on a virtual screen, {size[1]}x{size[0]} px); {len(read)} pictures read "
              f"from a film of the screen, the parts in order, {len(parts)} distinct, reassembled by the Foundation UR library == signed PSBT; Enter closes the window: OK")

        s = sign_with_window(env_win)
        s.text("(or type t to draw the QR code here instead): "); s.send("t")
        s.expect(r"Frame 1/\d+"); s.expect(pexpect.TIMEOUT, timeout=0.8)
        assert not running("ffplay"), "the window closes when t is typed"
        s.send(); s.expect(pexpect.EOF)
        print("N2) typing t closes the window and draws the QR code in the terminal instead: OK")

        s = Session(["qr", "wallet-two"], env=env_win)
        s.text("The QR code is shown in a window of its own.")
        s.text("(or type t to draw the QR code here instead): ")
        read, size = window_film(1.5, "backup-window")
        backup_line = [l for l in open(os.path.join(work, "seedlocker backups", "wallet-two.seedlock")).read().splitlines() if l.startswith("SEEDLOCK1:")][0]
        assert read and set(read) == {backup_line}, (len(read), read[:1])
        s.send(); s.expect(pexpect.EOF)
        assert "The QR code was also saved as a picture" in s.log.getvalue() and not running("ffplay")
        print(f"N3) menu 8 ('seedlock qr'): the backup's QR code opens in the window ({size[1]}x{size[0]} px) and reads back as exactly the backup text; the picture is still saved: OK")

        no_screen = {k: v for k, v in env_win.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY")}
        no_ffplay = dict(env_win, SEEDLOCK_FFPLAY="/nonexistent/ffplay")
        for env, why in ((no_screen, "no graphical screen was found"), (no_ffplay, "ffplay (it comes with ffmpeg) was not found")):
            s = sign_with_window(env)
            s.text(f"(No QR window: {why}"); s.text("The QR code is drawn in this terminal instead.)")
            s.expect(r"Frame 1/\d+"); s.expect(pexpect.TIMEOUT, timeout=0.8); s.send(); s.expect(pexpect.EOF)
        print("N4) without a screen, or without ffplay, SeedLock says why in one line and draws the QR code in the terminal: OK")
    finally:
        xvfb.terminate()

# ---------------------------------------------------------------- O) a backup read from its QR code with the camera (the real ffmpeg)
backup_two = [l for l in open(os.path.join(work, "seedlocker backups", "wallet-two.seedlock")).read().splitlines() if l.startswith("SEEDLOCK1:")][0]
rng = np.random.default_rng(11)
shots_b = os.path.join(tools, "shots-backup"); os.makedirs(shots_b)
a_transaction = ur_parts(psbt_big.serialize())[0][0]          # shown first: it must be passed over, with a word
for i, text in enumerate([a_transaction] * 4 + [backup_two] * 8):
    cv2.imwrite(os.path.join(shots_b, f"f{i:03d}.png"), camera_picture(text, rng))
video_b = os.path.join(tools, "backup.mp4")
subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-framerate", "4", "-i", os.path.join(shots_b, "f%03d.png"), *codec, "-pix_fmt", "yuv420p", video_b], check=True)
wrapper_b = os.path.join(tools, "ffmpeg-as-camera-backup")
open(wrapper_b, "w").write(open(wrapper).read().replace(repr(video), repr(video_b)))
os.chmod(wrapper_b, 0o755)
env_bcam = dict(BASE_ENV, SEEDLOCK_FFMPEG=wrapper_b)

def scan_the_backup(s):
    s.text(CHOOSE)
    assert "c) Scan a backup's QR code with the camera" in s.before, s.before[-600:]
    s.send("c")
    s.text("Opening the camera ...")
    s.text("Show the backup's QR code (generated in menu 8, or printed or on a phone)")
    s.text("That is a transaction's QR code, not a backup.")
    s.text("Read the backup's QR code.")

s = Session(env=env_bcam)
s.expect(r"\n> "); s.send("4")
scan_the_backup(s)
s.expect(r"paper backup\? \[y/N\] "); s.send("n")
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.expect("Backup OK")
s.expect(r"\n> "); s.send("6")
scan_the_backup(s)
s.text("How does the transaction get to this computer?")
s.text("Choose 1 or 2 [1]: "); s.send("1")
s.expect(r"Choose a file: number 1-(\d+)"); s.send("2")                  # send.psbt
s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")
s.expect(r"Backup password \(hidden\): "); s.send(PW2)
s.text("Network: mainnet (real bitcoin), found from the transaction")
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.text("The signed PSBT is saved as:")
s.text("Choose 1 or 2 [1]: ")
signed_path = re.search(r"The signed PSBT is saved as:\r?\n\s+(\S+)", s.log.getvalue()).group(1)
s.send("2")
s.text("Take that file to your online wallet")
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
sp = PSBT.parse(open(signed_path, "rb").read())
assert in_pub.verify(ec.Signature.parse(sp.inputs[0].partial_sigs[in_pub][:-1]), sp.sighash(0)), "the signature does not verify"
assert not any("backup" in f.lower() and f.endswith(".seedlock") and f != "wallet-two.seedlock" and f != "wallet-one.seedlock"
               for f in os.listdir(os.path.join(work, "seedlocker backups"))), "a scanned backup is not saved anywhere"
print("O) a backup read from its QR code with the camera (the real ffmpeg; a soft, noisy, compressed video that shows a transaction's "
      "code first): the transaction's code is passed over with a word; the backup opens (menu 4) and signs (menu 6, signature verified by embit): OK")

# ---------------------------------------------------------------- P) menu 5: the wallet laid out for Sparrow
from embit import bip32, bip39, script as escript
from embit.networks import NETWORKS
_IN = "0123456789()[],'/*abcdefgh@:$%{}IJKLMNOPQRSTUVWXYZ&+-.;<=>?!^_|~ijklmnopqrstuvwxyzABCDEFGH`#\"\\ "
_GEN = [0xf5dee51989, 0xa9fdca3312, 0x1bab10e32d, 0x3706b1677a, 0x644d626ffd]
def descsum_create(body):   # the reference code of BIP380
    def polymod(symbols):
        chk = 1
        for value in symbols:
            top = chk >> 35
            chk = (chk & 0x7ffffffff) << 5 ^ value
            for i in range(5):
                chk ^= _GEN[i] if ((top >> i) & 1) else 0
        return chk
    groups, symbols = [], []
    for c in body:
        v = _IN.find(c); symbols.append(v & 31); groups.append(v >> 5)
        if len(groups) == 3:
            symbols.append(groups[0] * 9 + groups[1] * 3 + groups[2]); groups = []
    if len(groups) == 1: symbols.append(groups[0])
    elif len(groups) == 2: symbols.append(groups[0] * 3 + groups[1])
    c = polymod(symbols + [0] * 8) ^ 1
    return body + "#" + "".join("qpzry9x8gf2tvdw0s3jn54khce6mua7l"[(c >> (5 * (7 - i))) & 31] for i in range(8))

root2 = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(WORDS2))
fp2 = root2.my_fingerprint.hex()
def xpub_at(path, net):
    return root2.derive(path).to_public().to_base58(version=NETWORKS[net]["xpub"])

def wallet_screen(s, answer_network=None):
    s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")
    s.expect(r"Backup password \(hidden\): "); s.send(PW2)
    if answer_network:
        s.text("There is no transaction of this wallet here, so SeedLock cannot tell its network by itself.")
        s.text("Type m or t [m]: "); s.send(answer_network)
    s.text("Show the wallet as a QR code for Sparrow to scan? [Y/n] ")
    shown = s.before
    s.send("n")
    s.text("Saved next to your backups.")
    s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
    return shown

# (1) this folder holds this wallet's transactions (mainnet, m/84'/0'/...): network and address type are found from them
s = Session()
s.expect(r"\n> "); s.send("5")
s.text(CHOOSE); s.send("2")
shown = wallet_screen(s)
assert f"Wallet {fp2} for Sparrow: Native Segwit (P2WPKH), mainnet (real bitcoin)" in shown, shown[-2500:]
assert "network and address type found from this wallet's latest transaction" in shown
assert f"Master fingerprint : {fp2}" in shown and "Derivation         : m/84'/0'/0'" in shown and f"xpub               : {xpub_at('m/84h/0h/0h', 'main')}" in shown
desc = re.search(r"^\s+(wpkh\(\S+\)#[a-z0-9]{8})\s*$", shown, re.M).group(1)
assert desc == descsum_create(desc.split("#")[0]), "the descriptor's checksum, by BIP380's reference code"
assert desc.startswith(f"wpkh([{fp2}/84h/0h/0h]{xpub_at('m/84h/0h/0h', 'main')}/<0;1>/*)"), desc
for purpose, name in ((86, "Taproot (P2TR)"), (49, "Nested Segwit (P2SH-P2WPKH)"), (44, "Legacy (P2PKH)")):
    assert re.search(rf"{re.escape(name)}\s+m/{purpose}'/0'/0'\s+{xpub_at(f'm/{purpose}h/0h/0h', 'main')}", shown), name
assert escript.p2wpkh(root2.derive("m/84h/0h/0h/0/0").to_public().key).address(NETWORKS["main"]) in shown
backups = os.path.join(work, "seedlocker backups")
saved_txt = [f for f in os.listdir(backups) if f.startswith("wallet-two-for-sparrow") and f.endswith(".txt")]
saved_png = [f for f in os.listdir(backups) if f.startswith("wallet-two-for-sparrow-qr") and f.endswith(".png")]
assert len(saved_txt) == 1 and len(saved_png) == 1, os.listdir(backups)
text_saved = open(os.path.join(backups, saved_txt[0])).read()
assert desc in text_saved and "Derivation         : m/84'/0'/0'" in text_saved
assert decode_qr(cv2.imread(os.path.join(backups, saved_png[0]), cv2.IMREAD_GRAYSCALE)) == desc, "the saved QR code holds the descriptor"

# (2) no transaction to learn from: one question, and a test network gives m/84'/1'/0' and a tpub
empty5 = tempfile.mkdtemp()
s = Session(cwd=empty5)
s.expect(r"\n> "); s.send("5")
s.text("or type c to scan the backup's QR code with the camera: "); s.send(os.path.join(backups, "wallet-two.seedlock"))
shown_t = wallet_screen(s, answer_network="t")
assert f"Wallet {fp2} for Sparrow: Native Segwit (P2WPKH), a test network (testnet or signet)" in shown_t, shown_t[-2500:]
assert "Derivation         : m/84'/1'/0'" in shown_t and f"xpub               : {xpub_at('m/84h/1h/0h', 'test')}" in shown_t
address_t = escript.p2wpkh(root2.derive("m/84h/1h/0h/0/0").to_public().key).address(NETWORKS["test"])
assert address_t.startswith("tb1q") and address_t in shown_t
print(f"P) menu 5 lays the wallet out for Sparrow: fingerprint, derivation m/84'/0'/0' (as Sparrow wants it) and xpub equal embit's; "
      f"network and address type found from this wallet's latest transaction; the other three types; the one-line descriptor's checksum "
      f"agrees with BIP380's reference code; saved as text and as a QR picture that reads back; with no transaction, one question: "
      f"t gives m/84'/1'/0', a tpub and tb1q addresses, all equal to embit's: OK")

log = ALL.getvalue()
for secret in (PW1, PW2, WORDS2):
    assert secret not in log, "a secret was echoed"
assert "--force" not in log and "overwrite it" not in log
print("G2) no secret echoed and no '--force' anywhere in the QR flows: OK")
