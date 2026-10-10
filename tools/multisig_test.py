#!/usr/bin/env python3
"""Multisig (2 of 3, native SegWit P2WSH) end to end, checked with an independent Bitcoin library (embit).

Nothing SeedLock computes is trusted here: embit derives the three cosigners' keys, writes the wallet's output
descriptor the way Sparrow exports it (the checksum by BIP380's reference code) and builds the transactions.
SeedLock only shows keys, registers the wallet and signs; embit checks the results.

  A) menu 10, 1: this seed's cosigner key (fingerprint, m/48'/1'/0'/2', tpub) equals embit's.
  B) menu 10, 2: registering from Sparrow's exported file, dropped in: a wrong path and a changed checksum are
     refused; the first receive address shown equals embit's (built by hand and by embit's descriptor engine);
     the same wallet again is "already registered"; menu 10, 3 lists it.
  C) menu 6 with cosigner A: the review names the wallet, verifies the change from all its keys and says one
     more signature is needed; then cosigner B on the command line. embit verifies both signatures over its own
     BIP143 signature hash (the witness script as script code) and puts them in the order OP_CHECKMULTISIG needs.
  D) a "change" output that holds cosigner A's key but two keys of an attacker: shown as leaving the wallet,
     with a warning, and declining leaves nothing signed.
  E) menu 10, 1's QR code of the cosigner key, read with urtypes (the library SeedSigner uses for these codes):
     a UR crypto-account whose bytes equal what urtypes itself writes for that key.
  F) a nested SegWit wallet (sh(wsh(sortedmulti)), m/48'/1'/0'/1') registered by scanning an animated
     crypto-output QR code written exactly the way Sparrow writes it (its SettingsController and hummingbird:
     "is-private: false", coin info, depth, each key named by its keystore label, keys in Sparrow's order),
     made with urtypes and shown through the real ffmpeg; two cosigners sign; embit checks.
  G) a legacy wallet with its keys in a fixed order (sh(multi), m/45'): registered from a file, its network
     taken from the registration, two cosigners sign over the legacy signature hash; embit checks.
  H) menu 10, 1 with the backup read from its QR code (camera, the real ffmpeg): a QR code holds no name, so the
     backup is matched to its file in the backups folder and the keystore is named after it; a backup that is not
     there makes SeedLock ask for the name, which then names the keystore and the saved files.
  I) BBQr, as Sparrow shows codes when BBQr is chosen in its QR window, made by Coinkite's own reference code: a
     wallet registered from its descriptor as a compressed BBQr text series (menu 10, 2), and its transaction
     scanned as a compressed BBQr PSBT series and signed (menu 6); embit verifies the signature.
  J) a whole wallet in ONE static QR code, as Sparrow shows it at medium density (UR, and BBQr), drawn as Sparrow
     draws it and photographed on a screen (the screen's pixel pattern, glare, soft focus, JPEG): registered
     through the real ffmpeg; while a damaged copy of the code is in view, SeedLock says it sees a code it cannot
     read and what helps.

Needs: pip install pexpect embit urtypes segno opencv-python-headless numpy, ffmpeg, and Foundation Devices'
       UR library (git clone https://github.com/Foundation-Devices/foundation-ur-py)
Run:   python3 tools/multisig_test.py target/release/seedlock --ur-lib foundation-ur-py [--bbqr-lib BBQr/python]
       (--bbqr-lib: Coinkite's reference code, git clone https://github.com/coinkite/BBQr, for section I;
        it needs pyqrcode)
"""
import argparse, hashlib, io, os, re, shutil, subprocess, sys, tempfile

import pexpect
from embit import bip32, bip39, ec, script
from embit.descriptor import Descriptor
from embit.networks import NETWORKS
from embit.psbt import PSBT, DerivationPath
from embit.transaction import Transaction, TransactionInput, TransactionOutput

ap = argparse.ArgumentParser()
ap.add_argument("bin")
ap.add_argument("--ur-lib", required=True, help="a checkout of foundation-ur-py")
ap.add_argument("--bbqr-lib", help="the python folder of a checkout of github.com/coinkite/BBQr (for section I)")
opts = ap.parse_args()
sys.path.insert(0, os.path.abspath(opts.ur_lib))
import cv2, numpy as np, segno
from ur.ur import UR
from ur.ur_decoder import URDecoder
from ur.ur_encoder import UREncoder
from urtypes.crypto.account import Account
from urtypes.crypto.hd_key import CoinInfo, HDKey
from urtypes.crypto.keypath import Keypath, PathComponent
from urtypes.crypto.multi_key import MultiKey
from urtypes.crypto.output import SCRIPT_EXPRESSION_TAG_MAP, Output
BIN = os.path.abspath(opts.bin)
FFMPEG = shutil.which("ffmpeg")
work = tempfile.mkdtemp()
PW = "Correct-Horse-Battery-Staple"
SEEDS = {
    "alice": "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    "bob": "legal winner thank year wave sausage worth useful legal winner thank yellow",
    "carol": "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
}
BACKUPS = os.path.join(work, "seedlocker backups")
COSIGNERS = os.path.join(BACKUPS, "cosigner keys")
WALLETS = os.path.join(BACKUPS, "multisig wallets")
TEST = NETWORKS["test"]

# the same stand-ins as the other scripts: an offline routing table, no windows, no browser
for inherited in ("TERM_PROGRAM", "SEEDLOCK_QR_STYLE"):
    os.environ.pop(inherited, None)
net = tempfile.mkdtemp()
open(os.path.join(net, "route"), "w").write("Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n")
open(os.path.join(net, "ipv6_route"), "w").write("")
os.environ.update(SEEDLOCK_TEST_NET_DIR=net, SEEDLOCK_NO_QR_WINDOW="1", SEEDLOCK_NO_BROWSER="1")


class Session:
    def __init__(self):
        self.log = io.StringIO()
        self.c = pexpect.spawn(BIN, [], encoding="utf-8", timeout=120, dimensions=(70, 160), cwd=work)
        self.c.logfile_read = self

    def write(self, t): self.log.write(t)
    def flush(self): pass
    def expect(self, pattern, **kw): return self.c.expect(pattern, **kw)
    def text(self, exact): return self.c.expect(re.escape(exact))
    def send(self, t=""): self.c.sendline(t)
    def __getattr__(self, name): return getattr(self.c, name)


def run(args, stdin):
    return subprocess.run([BIN, *args], input=stdin, text=True, capture_output=True, cwd=work)


def descsum_create(body):   # the reference code of BIP380
    charset = "0123456789()[],'/*abcdefgh@:$%{}IJKLMNOPQRSTUVWXYZ&+-.;<=>?!^_|~ijklmnopqrstuvwxyzABCDEFGH`#\"\\ "
    gen = [0xf5dee51989, 0xa9fdca3312, 0x1bab10e32d, 0x3706b1677a, 0x644d626ffd]
    def polymod(symbols):
        chk = 1
        for value in symbols:
            top = chk >> 35
            chk = (chk & 0x7ffffffff) << 5 ^ value
            for i in range(5):
                chk ^= gen[i] if ((top >> i) & 1) else 0
        return chk
    groups, symbols = [], []
    for c in body:
        v = charset.find(c); symbols.append(v & 31); groups.append(v >> 5)
        if len(groups) == 3:
            symbols.append(groups[0] * 9 + groups[1] * 3 + groups[2]); groups = []
    if len(groups) == 1: symbols.append(groups[0])
    elif len(groups) == 2: symbols.append(groups[0] * 3 + groups[1])
    c = polymod(symbols + [0] * 8) ^ 1
    return body + "#" + "".join("qpzry9x8gf2tvdw0s3jn54khce6mua7l"[(c >> (5 * (7 - i))) & 31] for i in range(8))


# ---------------------------------------------------------------- the three cosigners, by embit
os.makedirs(BACKUPS)
for name, words in SEEDS.items():
    p = run(["encrypt", "--out", os.path.join(BACKUPS, name + ".seedlock"), "--kdf", "light"], f"{words}\n{PW}\n")
    assert p.returncode == 0, p.stderr
H = 0x80000000
ACCOUNT = [48 + H, 1 + H, 0 + H, 2 + H]
roots = {n: bip32.HDKey.from_seed(bip39.mnemonic_to_seed(w)) for n, w in SEEDS.items()}
fps = {n: r.my_fingerprint.hex() for n, r in roots.items()}
tpubs = {n: r.derive(ACCOUNT).to_public().to_base58(version=TEST["xpub"]) for n, r in roots.items()}


def pub(name, chain, index):
    return roots[name].derive(ACCOUNT + [chain, index]).key.get_public_key()


def vault_script(chain, index):
    keys = sorted((pub(n, chain, index) for n in SEEDS), key=lambda k: k.sec())
    return script.multisig(2, keys), keys


def descriptor(tail):
    keys = ",".join(f"[{fps[n]}/48h/1h/0h/2h]{tpubs[n]}/{tail}" for n in SEEDS)
    return descsum_create(f"wsh(sortedmulti(2,{keys}))")


# ---------------------------------------------------------------- A) menu 10, 1: this seed's cosigner key
s = Session()
s.expect(r"\n> "); s.send("10")
s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("1")
s.text("Which seed (backup) do you want to add as a cosigner?")
s.text('drop your backup file and press "Enter": '); s.send("alice")
s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")
s.expect(r"Backup password \(hidden\): "); s.send(PW)
s.text("SeedLock cannot tell by itself which network the multisig wallet is for.")
s.text("Type m or t [m]: "); s.send("t")
s.text("Which kind of multisig wallet is it (Sparrow's \"Script Type\")?")
s.text("Choose 1, 2 or 3 [1]: "); s.send("")                             # Native Segwit (P2WSH), the usual one
s.text("Show this seed's key as a QR code for Sparrow to scan? [Y/n] ")
shown = s.before                                                       # the key laid out for Sparrow, just above the question
s.send("n")
s.text('Saved in the "cosigner keys" folder of your backups')
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
for wanted in (f"Master fingerprint : {fps['alice']}", "Derivation         : m/48'/1'/0'/2'", f"xpub               : {tpubs['alice']}",
               f"[{fps['alice']}/48h/1h/0h/2h]{tpubs['alice']}", 'Script Type\r\n"Native Segwit (P2WSH)"',
               'choose "Watch Only Wallet"', 'button next to the xpub field ("Scan a QR code")', 'names the keystore "alice"'):
    assert wanted in shown, (wanted, shown[-1600:])
assert "SeedSigner" not in shown and "xPub /" not in shown, "Sparrow 2.5's own button names, not another device's option"
assert os.path.isfile(os.path.join(COSIGNERS, "alice-cosigner.txt")) and os.path.isfile(os.path.join(COSIGNERS, "alice-cosigner-qr.png"))
print("A) menu 10, 1: the cosigner key laid out for Sparrow (fingerprint, derivation m/48'/1'/0'/2', tpub) equals embit's, and is saved: OK")

# ---------------------------------------------------------------- B) menu 10, 2 and 3: register the wallet from Sparrow's file
export = os.path.join(work, "exported from sparrow", "vault.txt")        # a folder name with spaces
os.makedirs(os.path.dirname(export))
open(export, "w").write("# Receive and change descriptor (BIP389)\n" + descriptor("<0;1>/*") +
                        "\n\n# Receive descriptor\n" + descriptor("0/*") + "\n# Change descriptor\n" + descriptor("1/*") + "\n")
by_hand = script.p2wsh(vault_script(0, 0)[0]).address(TEST)
by_engine = Descriptor.from_string(descriptor("0/*")).derive(0).address(TEST)
assert by_hand == by_engine, (by_hand, by_engine)

s = Session()
s.expect(r"\n> "); s.send("10")
s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("2")
s.text("(q to go back): "); s.send("/nonexistent/vault.txt")
s.text('There is no file at "/nonexistent/vault.txt".')
bad = descriptor("<0;1>/*")[:-1] + ("q" if descriptor("<0;1>/*")[-1] != "q" else "p")
s.text("(q to go back): "); s.send(bad)
s.text("That cannot be registered: the descriptor's checksum (after #) does not match")
s.text("(q to go back): "); s.send(f"'{export}' ")                     # dropped the Linux way
s.text("Multisig wallet: 2 of 3 signatures needed, Native Segwit (P2WSH), a test network (testnet or signet)")
s.expect(r"Its first receive address: (tb1q\w+)\r\n")
assert s.match.group(1) == by_hand, (s.match.group(1), by_hand)
s.text("Does it match, and are these the right cosigners? [y/N] "); s.send("y")
s.text("Name for this wallet [multisig-2-of-3]: "); s.send("vault")
s.text('Registered as "vault":')
s.expect(r"\n> "); s.send("10")
s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("2")
s.text("(q to go back): "); s.send(descriptor("<0;1>/*"))              # pasted this time: the same wallet
s.text('This multisig wallet is already registered, as "vault". Nothing was changed.')
s.expect(r"\n> "); s.send("10")
s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("3")
s.text('"vault": 2 of 3, Native Segwit (P2WSH), a test network (testnet or signet)')
s.text(f"first receive address: {by_hand}")
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
saved = open(os.path.join(WALLETS, "vault.multisig")).read()
assert descriptor("<0;1>/*") in saved, saved
print("B) menu 10, 2: registered from Sparrow's exported file (dropped in); a wrong path and a changed checksum are refused; "
      "the first address equals embit's (by hand and by its descriptor engine); again: already registered; menu 10, 3 lists it: OK")

# ---------------------------------------------------------------- C) sign: cosigner A in menu 6, then cosigner B on the command line
OUTSIDE = script.p2wpkh(ec.PrivateKey(bytes([9]) * 32).get_public_key())


def vault_psbt(change=None):
    ws_in, _ = vault_script(0, 0)
    ws_change, _ = vault_script(1, 0)
    change_ws, change_paths = change or (ws_change, {pub(n, 1, 0): (roots[n].my_fingerprint, ACCOUNT + [1, 0]) for n in SEEDS})
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex("dd" * 32), 0)],
                     vout=[TransactionOutput(60_000, OUTSIDE), TransactionOutput(39_000, script.p2wsh(change_ws))])
    p = PSBT(tx)
    p.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wsh(ws_in))
    p.inputs[0].witness_script = ws_in
    for n in SEEDS:
        p.inputs[0].bip32_derivations[pub(n, 0, 0)] = DerivationPath(roots[n].my_fingerprint, ACCOUNT + [0, 0])
    p.outputs[1].witness_script = change_ws
    for key, (fp, path) in change_paths.items():
        p.outputs[1].bip32_derivations[key] = DerivationPath(fp, path)
    return p


open(os.path.join(work, "vault.psbt"), "w").write(vault_psbt().to_string())
x, y = (ec.PrivateKey(bytes([k]) * 32).get_public_key() for k in (7, 8))
fake = script.multisig(2, sorted([pub("alice", 1, 0), x, y], key=lambda k: k.sec()))
open(os.path.join(work, "vault-attack.psbt"), "w").write(vault_psbt(change=(fake, {
    pub("alice", 1, 0): (roots["alice"].my_fingerprint, ACCOUNT + [1, 0]),
    x: (bytes.fromhex("11223344"), ACCOUNT + [1, 0]),
    y: (bytes.fromhex("55667788"), ACCOUNT + [1, 0]),
})).to_string())

s = Session()
s.expect(r"\n> "); s.send("6")
s.text('drop your backup file and press "Enter": '); s.send("alice")
s.text("How does the transaction get to this computer?")
s.text("Choose 1 or 2 [1]: "); s.send("1")
s.expect(r"Choose a file: number 1-\d+")
number = re.search(r"(\d+)\) vault\.psbt ", s.before).group(1)
s.send(number)
s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")
s.expect(r"Backup password \(hidden\): "); s.send(PW)
for wanted in ("Network: a test network (testnet or signet), found from the transaction",
               "yours, from the multisig wallet \"vault\" (m/48'/1'/0'/2'/0/0)",
               "change back to the multisig wallet \"vault\" (verified from all its cosigners' keys: m/48'/1'/0'/2'/1/0)",
               "Leaving your wallet: 0.00060000 BTC",
               "Multisig \"vault\", 2 of 3: this seed is cosigner",
               "1 more cosigner must sign it too before Sparrow can finalize it."):
    s.text(wanted)
s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
s.text("The signed PSBT is saved as:")
s.text("Choose 1 or 2 [1]: "); s.send("2")
s.text("Take that file to your online wallet")
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
assert "WARNING" not in s.log.getvalue().split("TRANSACTION REVIEW")[1].split("Do you want to SIGN")[0]

o = run(["sign", os.path.join(BACKUPS, "bob.seedlock"), "--psbt", "vault.signed.psbt", "--yes"], f"{PW}\n")
assert o.returncode == 0, o.stderr
assert "That is enough: after this, Sparrow can finalize and broadcast the transaction." in o.stderr, o.stderr

final = PSBT.parse(open(os.path.join(work, "vault.signed.signed.psbt"), "rb").read())
ws_in, keys_in = vault_script(0, 0)
digest = final.tx.sighash_segwit(0, ws_in, 100_000)                   # BIP143, the witness script as script code
sigs = final.inputs[0].partial_sigs
assert set(sigs) == {pub("alice", 0, 0), pub("bob", 0, 0)}, list(sigs)
for key, sig in sigs.items():
    assert sig[-1] == 1, "SIGHASH_ALL"
    assert key.verify(ec.Signature.parse(sig[:-1]), digest), "a signature does not verify"
ordered = [sigs[k] for k in keys_in if k in sigs]                     # OP_CHECKMULTISIG takes them in the script's key order
witness = [b"", *ordered[:2], ws_in.data]
assert len(ordered) == 2 and hashlib.sha256(ws_in.data).digest() == final.inputs[0].witness_utxo.script_pubkey.data[2:]
print("C) menu 6 with cosigner A: the review names the wallet, verifies the change from all its keys, says 1 more signature is "
      "needed; cosigner B on the command line: enough. embit verifies both signatures over its own BIP143 hash, and orders them "
      f"for OP_CHECKMULTISIG (a {len(witness)}-item witness whose script hashes to the input's program): OK")

# ---------------------------------------------------------------- D) the swapped-change attack
o = run(["sign", os.path.join(BACKUPS, "alice.seedlock"), "--psbt", "vault-attack.psbt"], f"{PW}\nn\n")
review = o.stderr
assert review.count("SENT TO SOMEONE ELSE") == 2, review
assert "output #1 claims to be yours, but its address does not match your seed" in review, review
assert "Leaving your wallet: 0.00099000 BTC" in review and "change back to the multisig wallet" not in review, review
assert not os.path.exists(os.path.join(work, "vault-attack.signed.psbt")), "declining must leave nothing signed"
print("D) a \"change\" output with cosigner A's key but two keys of an attacker: shown as SENT TO SOMEONE ELSE with a warning, "
      "and declining leaves nothing signed: OK")

def decode_qr(img):
    detectors = ([cv2.QRCodeDetectorAruco] if hasattr(cv2, "QRCodeDetectorAruco") else []) + [cv2.QRCodeDetector]
    for make in detectors:
        text, _, _ = make().detectAndDecode(img)
        if text:
            return text
    return ""


def origin(path, fp):
    return Keypath([PathComponent(i & ~H, bool(i & H)) for i in path], fp, len(path))


def ur_hdkey(name, path, label=None):
    key = roots[name].derive(path).to_public()
    return HDKey({"key": key.key.sec(), "chain_code": key.chain_code, "use_info": CoinInfo(0, 1), "origin": origin(path, roots[name].my_fingerprint),
                  "parent_fingerprint": key.fingerprint, "name": label})


def sparrow_hdkey(name, path, label):
    """A key the way Sparrow writes it in its descriptor QR code: new CryptoHDKey(false, pubkey, chain code,
    CryptoCoinInfo(BITCOIN, TESTNET), CryptoKeypath(path, fingerprint, depth), null, parent fingerprint, label, null)."""
    key = roots[name].derive(path).to_public()
    return HDKey({"private_key": False, "key": key.key.sec(), "chain_code": key.chain_code, "use_info": CoinInfo(0, 1),
                  "origin": origin(path, roots[name].my_fingerprint), "parent_fingerprint": key.fingerprint, "name": label})


# ---------------------------------------------------------------- E) the cosigner key as SeedSigner's QR code
picture = cv2.imread(os.path.join(COSIGNERS, "alice-cosigner-qr.png"), cv2.IMREAD_GRAYSCALE)
code = decode_qr(picture)
assert code.startswith("UR:CRYPTO-ACCOUNT/"), code[:40]
assert code in open(os.path.join(COSIGNERS, "alice-cosigner.txt")).read()
decoder = URDecoder()
decoder.receive_part(code.lower())
assert decoder.is_complete()
from_seedlock = bytes(decoder.result_message().cbor)
account = Account.from_cbor(from_seedlock)
assert account.master_fingerprint.hex() == fps["alice"]
assert account.output_descriptors[0].descriptor(include_checksum=False) == f"wsh([{fps['alice']}/48'/1'/0'/2']{tpubs['alice']})", \
    account.output_descriptors[0].descriptor(include_checksum=False)
expected = Account(roots["alice"].my_fingerprint, [Output([SCRIPT_EXPRESSION_TAG_MAP[401]], ur_hdkey("alice", ACCOUNT, "alice"))]).to_cbor()
assert account.output_descriptors[0].hd_key().name == "alice", "the keystore label Sparrow takes: the backup's name"
assert from_seedlock == expected, "SeedLock's crypto-account must be byte for byte what urtypes writes"
print("E) menu 10, 1's QR code (read back from the saved picture) is a UR crypto-account that urtypes reads as "
      "wsh([fingerprint/48'/1'/0'/2']tpub) named \"alice\" (the backup's name), byte for byte what urtypes itself writes for that key: OK")

# ---------------------------------------------------------------- F) nested SegWit, registered from an animated crypto-output QR code
NESTED = [48 + H, 1 + H, 0 + H, 1 + H]
first_receive = {n: roots[n].derive(NESTED + [0, 0]).key.get_public_key().sec() for n in SEEDS}
in_sparrows_order = sorted(SEEDS, key=lambda n: first_receive[n])      # Sparrow sorts the keys by their first receive key
output = Output([SCRIPT_EXPRESSION_TAG_MAP[400], SCRIPT_EXPRESSION_TAG_MAP[401], SCRIPT_EXPRESSION_TAG_MAP[407]],
                MultiKey(2, [], [sparrow_hdkey(n, NESTED, f"Keystore {i + 1}") for i, n in enumerate(in_sparrows_order)]))
assert b"\x02\xf4" in output.to_cbor(), "each key says is-private: false, as Sparrow's do"
encoder = UREncoder(UR("crypto-output", bytearray(output.to_cbor())), 70)
first = encoder.next_part()
n_parts = int(re.match(r"ur:[^/]+/\d+-(\d+)/", first).group(1))
parts = [first] + [encoder.next_part() for _ in range(2 * n_parts - 1)]
assert n_parts > 3, "an animated code"
shots = tempfile.mkdtemp()
for i, text in enumerate(parts):
    q = segno.make(text.upper(), error="l", micro=False, boost_error=False)
    m = np.pad(np.array([[0 if b else 255 for b in row] for row in q.matrix], np.uint8), 4, constant_values=255)
    frame = np.full((720, 1280), 200, np.uint8)
    frame[140:580, 420:860] = cv2.resize(m, (440, 440), interpolation=cv2.INTER_NEAREST)
    cv2.imwrite(os.path.join(shots, f"f{i:03d}.png"), frame)
video = os.path.join(shots, "descriptor.mp4")
codec = ["-c:v", "libx264", "-crf", "28"] if b"libx264" in subprocess.run([FFMPEG, "-encoders"], capture_output=True).stdout else ["-c:v", "mpeg4", "-q:v", "6"]
subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-framerate", "4", "-i", os.path.join(shots, "f%03d.png"), "-vf", "fps=12", *codec, "-pix_fmt", "yuv420p", video], check=True)
wrapper = os.path.join(shots, "ffmpeg-as-camera")
open(wrapper, "w").write(f"""#!/usr/bin/env python3
import os, sys
real, video, args = {FFMPEG!r}, {video!r}, sys.argv[1:]
if "-version" in args:
    os.execv(real, [real, "-version"])
os.execv(real, [real, "-hide_banner", "-loglevel", "error", "-nostdin", "-re", "-stream_loop", "-1", "-i", video] + args[args.index("-an"):])
""")
os.chmod(wrapper, 0o755)
nested_keys = ",".join(f"[{fps[n]}/48h/1h/0h/1h]{roots[n].derive(NESTED).to_public().to_base58(version=TEST['xpub'])}/0/*" for n in SEEDS)
nested_first = Descriptor.from_string(f"sh(wsh(sortedmulti(2,{nested_keys})))").derive(0).address(TEST)

os.environ["SEEDLOCK_FFMPEG"] = wrapper
s = Session()
os.environ.pop("SEEDLOCK_FFMPEG")
s.expect(r"\n> "); s.send("10")
s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("2")
s.text("or type c to scan Sparrow's QR code with the camera (q to go back): "); s.send("c")
s.text("Opening the camera ...")
s.text("Read the wallet's QR code.")
s.text("Multisig wallet: 2 of 3 signatures needed, Nested Segwit (P2SH-P2WSH), a test network (testnet or signet)")
s.expect(r"Its first receive address: (2\w+)\r\n")
assert s.match.group(1) == nested_first, (s.match.group(1), nested_first)
s.text("Does it match, and are these the right cosigners? [y/N] "); s.send("y")
s.text("Name for this wallet [multisig-2-of-3]: "); s.send("nested")
s.text('Registered as "nested":')
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)


def nested_psbt():
    def script_at(chain, index):
        keys = sorted((roots[n].derive(NESTED + [chain, index]).key.get_public_key() for n in SEEDS), key=lambda k: k.sec())
        return script.multisig(2, keys)
    ws_in, ws_change = script_at(0, 0), script_at(1, 0)
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex("ee" * 32), 0)],
                     vout=[TransactionOutput(60_000, OUTSIDE), TransactionOutput(39_000, script.p2sh(script.p2wsh(ws_change)))])
    p = PSBT(tx)
    p.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2sh(script.p2wsh(ws_in)))
    p.inputs[0].redeem_script, p.inputs[0].witness_script = script.p2wsh(ws_in), ws_in
    p.outputs[1].redeem_script, p.outputs[1].witness_script = script.p2wsh(ws_change), ws_change
    for n in SEEDS:
        p.inputs[0].bip32_derivations[roots[n].derive(NESTED + [0, 0]).key.get_public_key()] = DerivationPath(roots[n].my_fingerprint, NESTED + [0, 0])
        p.outputs[1].bip32_derivations[roots[n].derive(NESTED + [1, 0]).key.get_public_key()] = DerivationPath(roots[n].my_fingerprint, NESTED + [1, 0])
    return p, ws_in


def sign_twice(name, psbt_text, expect_in_review):
    open(os.path.join(work, name + ".psbt"), "w").write(psbt_text)
    o = run(["sign", os.path.join(BACKUPS, "alice.seedlock"), "--psbt", name + ".psbt", "--yes"], f"{PW}\n")
    assert o.returncode == 0, o.stderr
    for wanted in expect_in_review:
        assert wanted in o.stderr, (wanted, o.stderr[-1500:])
    o = run(["sign", os.path.join(BACKUPS, "bob.seedlock"), "--psbt", name + ".signed.psbt", "--yes"], f"{PW}\n")
    assert o.returncode == 0 and "That is enough" in o.stderr, o.stderr
    return PSBT.parse(open(os.path.join(work, name + ".signed.signed.psbt"), "rb").read())


final, ws_in = nested_psbt()
final = sign_twice("nested", final.to_string(), ['yours, from the multisig wallet "nested"', 'change back to the multisig wallet "nested"'])
digest = final.tx.sighash_segwit(0, ws_in, 100_000)
sigs = final.inputs[0].partial_sigs
assert len(sigs) == 2 and all(k.verify(ec.Signature.parse(v[:-1]), digest) for k, v in sigs.items())
print(f"F) a nested SegWit wallet registered by scanning an animated crypto-output code written exactly as Sparrow writes it "
      f"(is-private: false, labels; {n_parts} parts, through the real ffmpeg); its first address equals embit's; two cosigners "
      "sign and embit verifies both signatures: OK")

# ---------------------------------------------------------------- G) legacy, keys in a fixed order, m/45'
LEGACY = [45 + H]
legacy_keys = ",".join(f"[{fps[n]}/45h]{roots[n].derive(LEGACY).to_public().to_base58(version=TEST['xpub'])}/<0;1>/*" for n in SEEDS)
open(os.path.join(work, "legacy.txt"), "w").write(descsum_create(f"sh(multi(2,{legacy_keys}))") + "\n")


def legacy_script(chain, index):
    return script.multisig(2, [roots[n].derive(LEGACY + [chain, index]).key.get_public_key() for n in SEEDS])   # in this order


legacy_first = script.p2sh(legacy_script(0, 0)).address(TEST)
s = Session()
s.expect(r"\n> "); s.send("10")
s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("2")
s.text("(q to go back): "); s.send(os.path.join(work, "legacy.txt"))
s.text("Multisig wallet: 2 of 3 signatures needed, Legacy (P2SH) with its keys in a fixed order, a test network (testnet or signet)")
s.expect(r"Its first receive address: (2\w+)\r\n")
assert s.match.group(1) == legacy_first, (s.match.group(1), legacy_first)
s.text("Does it match, and are these the right cosigners? [y/N] "); s.send("y")
s.text("Name for this wallet [multisig-2-of-3]: "); s.send("legacy")
s.text('Registered as "legacy":')
s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)

redeem_in, redeem_change = legacy_script(0, 0), legacy_script(1, 0)
prev = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex("ab" * 32), 3)], vout=[TransactionOutput(100_000, script.p2sh(redeem_in))])
tx = Transaction(version=2, locktime=0, vin=[TransactionInput(prev.txid(), 0)],
                 vout=[TransactionOutput(60_000, OUTSIDE), TransactionOutput(39_000, script.p2sh(redeem_change))])
p = PSBT(tx)
p.inputs[0].non_witness_utxo = prev
p.inputs[0].redeem_script = redeem_in
p.outputs[1].redeem_script = redeem_change
for n in SEEDS:
    p.inputs[0].bip32_derivations[roots[n].derive(LEGACY + [0, 0]).key.get_public_key()] = DerivationPath(roots[n].my_fingerprint, LEGACY + [0, 0])
    p.outputs[1].bip32_derivations[roots[n].derive(LEGACY + [1, 0]).key.get_public_key()] = DerivationPath(roots[n].my_fingerprint, LEGACY + [1, 0])
final = sign_twice("legacy", p.to_string(), ["Network: a test network (testnet or signet), from the registered multisig wallet",
                                            'yours, from the multisig wallet "legacy"', 'change back to the multisig wallet "legacy"'])
digest = final.tx.sighash_legacy(0, redeem_in)
sigs = final.inputs[0].partial_sigs
assert len(sigs) == 2 and all(k.verify(ec.Signature.parse(v[:-1]), digest) for k, v in sigs.items())
print("G) a legacy wallet with its keys in a fixed order (sh(multi), m/45'): registered from a file, its first address equals "
      "embit's, its network taken from the registration; two cosigners sign and embit verifies both over the legacy signature hash: OK")

# ---------------------------------------------------------------- H) a backup read from its QR code: the keystore is named after it
def camera_showing(texts, tag):
    """A wrapper that makes the real ffmpeg play a video of these QR codes as if it were the camera."""
    shots = tempfile.mkdtemp(prefix=tag)
    for i, text in enumerate(texts):
        q = segno.make(text, error="m", micro=False, boost_error=False)
        m = np.pad(np.array([[0 if b else 255 for b in row] for row in q.matrix], np.uint8), 4, constant_values=255)
        frame = np.full((720, 1280), 200, np.uint8)
        frame[140:580, 420:860] = cv2.resize(m, (440, 440), interpolation=cv2.INTER_NEAREST)
        cv2.imwrite(os.path.join(shots, f"f{i:03d}.png"), frame)
    video = os.path.join(shots, "camera.mp4")
    # each code stays 250 ms but over 3 camera pictures (12 a second), as a real camera sees a screen: shown in only
    # one picture per loop, a code could be missed every time when reading a picture takes about one loop
    subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-framerate", "4", "-i", os.path.join(shots, "f%03d.png"), "-vf", "fps=12", *codec, "-pix_fmt", "yuv420p", video], check=True)
    w = os.path.join(shots, "ffmpeg-as-camera")
    open(w, "w").write(f"""#!/usr/bin/env python3
import os, sys
real, video, args = {FFMPEG!r}, {video!r}, sys.argv[1:]
if "-version" in args:
    os.execv(real, [real, "-version"])
os.execv(real, [real, "-hide_banner", "-loglevel", "error", "-nostdin", "-re", "-stream_loop", "-1", "-i", video] + args[args.index("-an"):])
""")
    os.chmod(w, 0o755)
    return w


def backup_code(path):
    return [l for l in open(path).read().splitlines() if l.startswith("SEEDLOCK1:")][0]


def share_scanned(wrapper, after_scan):
    """Menu 10, 1 with the backup scanned; `after_scan(s)` answers what comes before the passphrase question.
    Returns what the screen showed and the QR picture SeedLock saved."""
    before = set(os.listdir(COSIGNERS))
    os.environ["SEEDLOCK_FFMPEG"] = wrapper
    s = Session()
    os.environ.pop("SEEDLOCK_FFMPEG")
    s.expect(r"\n> "); s.send("10")
    s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("1")
    s.text("Which seed (backup) do you want to add as a cosigner?")
    s.text('drop your backup file and press "Enter": '); s.send("c")
    s.text("Read the backup's QR code.")
    after_scan(s)
    s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")
    s.expect(r"Backup password \(hidden\): "); s.send(PW)
    return s, before


def finish_share(s, before):
    s.text("Show this seed's key as a QR code for Sparrow to scan? [Y/n] ")
    shown = ANSI_RE.sub("", s.before)
    s.send("n")
    s.text('Saved in the "cosigner keys" folder of your backups')
    s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
    new = sorted(set(os.listdir(COSIGNERS)) - before)
    picture = [f for f in new if f.endswith(".png")]
    assert len(new) == 2 and len(picture) == 1, new
    code = decode_qr(cv2.imread(os.path.join(COSIGNERS, picture[0]), cv2.IMREAD_GRAYSCALE))
    d = URDecoder(); d.receive_part(code.lower())
    return shown, new, Account.from_cbor(bytes(d.result_message().cbor)).output_descriptors[0].hd_key().name


ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
# H1) alice's backup, whose file is in the backups folder
s, before = share_scanned(camera_showing([backup_code(os.path.join(BACKUPS, "alice.seedlock"))], "h1-"),
                          lambda s: s.text('(This QR code is the backup "alice" in your backups folder, so its name is used.)'))
shown, files, label = finish_share(s, before)
assert 'names the keystore "alice"' in shown and label == "alice", (label, shown[-600:])
assert all(f.startswith("alice-cosigner") for f in files), files
# H2) a backup kept somewhere else: SeedLock asks for its name
outside = os.path.join(tempfile.mkdtemp(), "paper.seedlock")
o = run(["encrypt", "--out", outside, "--kdf", "light"], f"zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong\n{PW}\n")
assert o.returncode == 0, o.stderr


def ask_the_name(s):
    s.text("A QR code does not hold the backup's name, and this backup is not in your backups folder.")
    s.text("What is this seed called? (It names the saved files and the keystore in Sparrow.) [seedlock-backup]: ")
    s.send("dave paper")


s, before = share_scanned(camera_showing([backup_code(outside)], "h2-"), ask_the_name)
s.text("Type m or t [m]: "); s.send("t")
s.text("Choose 1, 2 or 3 [1]: "); s.send("")
shown, files, label = finish_share(s, before)
assert 'names the keystore "dave paper"' in shown and label == "dave paper", (label, shown[-600:])
assert sorted(files) == ["dave paper-cosigner-qr.png", "dave paper-cosigner.txt"], files
print("H) menu 10, 1 with the backup read from its QR code (the real ffmpeg): matched to its file, the keystore is named "
      "\"alice\"; a backup kept elsewhere makes SeedLock ask, and \"dave paper\" names the keystore and the saved files: OK")

# ---------------------------------------------------------------- I) BBQr codes, made by Coinkite's reference code
if not opts.bbqr_lib:
    print("I) SKIPPED: BBQr needs Coinkite's reference code: git clone https://github.com/coinkite/BBQr, then --bbqr-lib BBQr/python")
else:
    sys.path.insert(0, os.path.abspath(opts.bbqr_lib))
    from bbqr.split import split_qrs
    # I1) a new 2-of-2 wallet (alice and bob), its descriptor as Sparrow writes it, shown as BBQr text (type U), compressed
    duo_keys = ",".join(f"[{fps[n]}/48h/1h/0h/2h]{tpubs[n]}/<0;1>/*" for n in ("alice", "bob"))
    duo_text = descsum_create(f"wsh(sortedmulti(2,{duo_keys}))")
    _, duo_parts = split_qrs(duo_text.encode(), "U", encoding="Z", min_split=2, min_version=1)
    assert len(duo_parts) >= 2 and all(p.startswith("B$ZU") for p in duo_parts), duo_parts
    duo_first = Descriptor.from_string(descsum_create(f"wsh(sortedmulti(2,{duo_keys.replace('<0;1>', '0')}))")).derive(0).address(TEST)
    os.environ["SEEDLOCK_FFMPEG"] = camera_showing(duo_parts, "i1-")
    s = Session()
    os.environ.pop("SEEDLOCK_FFMPEG")
    s.expect(r"\n> "); s.send("10")
    s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("2")
    s.text("or type c to scan Sparrow's QR code with the camera (q to go back): "); s.send("c")
    s.text("Read the wallet's QR code.")
    s.text("Multisig wallet: 2 of 2 signatures needed, Native Segwit (P2WSH), a test network (testnet or signet)")
    s.expect(r"Its first receive address: (tb1q\w+)\r\n")
    assert s.match.group(1) == duo_first, (s.match.group(1), duo_first)
    s.text("Does it match, and are these the right cosigners? [y/N] "); s.send("y")
    s.text("Name for this wallet [multisig-2-of-2]: "); s.send("duo")
    s.text('Registered as "duo":')
    s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)

    # I2) its transaction shown as a BBQr PSBT series (type P), compressed, scanned in menu 6 and signed by alice
    def duo_script(chain, index):
        return script.multisig(2, sorted((pub(n, chain, index) for n in ("alice", "bob")), key=lambda k: k.sec()))
    ws_in, ws_change = duo_script(0, 0), duo_script(1, 0)
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex("bb" * 32), 1)],
                     vout=[TransactionOutput(70_000, OUTSIDE), TransactionOutput(29_000, script.p2wsh(ws_change))])
    p = PSBT(tx)
    p.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wsh(ws_in))
    p.inputs[0].witness_script, p.outputs[1].witness_script = ws_in, ws_change
    for n in ("alice", "bob"):
        p.inputs[0].bip32_derivations[pub(n, 0, 0)] = DerivationPath(roots[n].my_fingerprint, ACCOUNT + [0, 0])
        p.outputs[1].bip32_derivations[pub(n, 1, 0)] = DerivationPath(roots[n].my_fingerprint, ACCOUNT + [1, 0])
    _, psbt_parts = split_qrs(p.serialize(), "P", encoding="Z", min_split=3, min_version=1)
    assert len(psbt_parts) >= 3 and all(x.startswith("B$ZP") for x in psbt_parts), psbt_parts
    os.environ["SEEDLOCK_FFMPEG"] = camera_showing(psbt_parts, "i2-")
    s = Session()
    os.environ.pop("SEEDLOCK_FFMPEG")
    s.expect(r"\n> "); s.send("6")
    s.text('drop your backup file and press "Enter": '); s.send("alice")
    s.text("How does the transaction get to this computer?")
    s.text("Choose 1 or 2 [1]: "); s.send("2")
    s.text("Received the whole transaction.")
    s.text("Does this wallet use a BIP39 passphrase? [y/N] "); s.send("n")
    s.expect(r"Backup password \(hidden\): "); s.send(PW)
    s.text('yours, from the multisig wallet "duo"')
    s.text('change back to the multisig wallet "duo"')
    s.expect(r"Do you want to SIGN this transaction\? \[y/N\] "); s.send("y")
    s.text("The signed PSBT is saved as:")
    s.text("Choose 1 or 2 [1]: ")
    signed_path = re.search(r"The signed PSBT is saved as:\r?\n\s+(\S.*\.psbt)", ANSI_RE.sub("", s.log.getvalue())).group(1).strip()
    s.send("2")
    s.text("Take that file to your online wallet")
    s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)
    signed = PSBT.parse(open(signed_path if os.path.isabs(signed_path) else os.path.join(work, signed_path), "rb").read())
    sig = signed.inputs[0].partial_sigs[pub("alice", 0, 0)]
    assert pub("alice", 0, 0).verify(ec.Signature.parse(sig[:-1]), signed.tx.sighash_segwit(0, ws_in, 100_000)), "alice's signature does not verify"
    print(f"I) BBQr made by Coinkite's reference code (Z, compressed): a wallet registered from its descriptor as a {len(duo_parts)}-part "
          f"text series (menu 10, 2), its transaction scanned as a {len(psbt_parts)}-part PSBT series and signed (menu 6), "
          "embit verifies the signature: OK")

# ---------------------------------------------------------------- J) a whole wallet in one static code, photographed on a screen
# J2's photo (noise seed, softness): the reader before its light-evening tries cannot read it, the current one does
J2_SEED, J2_BLUR = int(os.environ.get("J2_SEED", "8")), float(os.environ.get("J2_BLUR", "1.1"))
def sparrow_screen(text):
    """As Sparrow draws a QR code: ZXing, error correction L, margin 2, 580 pixels, whole pixels per module."""
    q = segno.make(text, error="l", micro=False, boost_error=False)
    m = np.array(q.matrix, dtype=np.uint8); n = m.shape[0] + 4; px = 580 // n
    img = np.pad(np.where(m == 1, 0, 255).astype(np.uint8), 2, constant_values=255)
    return cv2.resize(img, (n * px, n * px), interpolation=cv2.INTER_NEAREST), q.version


ALIGNMENT_2ND = {7: 22, 8: 24, 9: 26, 10: 28, 11: 30, 12: 32, 13: 34, 14: 26, 15: 26, 16: 26, 17: 30}   # second alignment mark (ISO 18004)


def damaged(img, version):
    """The drawn code with its data inverted in a band between its first two rows of alignment marks: far more
    errors than error correction L repairs, while the corners, timing lines and alignment marks stay intact, as in
    a code too soft to read. (Sparrow's drawing: 2 modules of margin, whole pixels per module.)"""
    n = 17 + 4 * version; px = img.shape[0] // (n + 4); out = img.copy()
    rows = slice((2 + 10) * px, (2 + ALIGNMENT_2ND[version] - 3) * px); cols = slice((2 + 10) * px, (2 + n - 12) * px)
    out[rows, cols] = 255 - out[rows, cols]
    return out


def screen_photo(img, seed=5, blur=1.3):
    """The code on a screen, photographed: the screen's subpixels, glare, a darker edge, soft focus, noise, JPEG."""
    r = np.random.default_rng(seed)
    img = img.astype(np.float32)
    big = cv2.resize(img, (img.shape[1] * 3, img.shape[0] * 3), interpolation=cv2.INTER_NEAREST)
    big = big * np.tile(np.array([1.0, 0.82, 0.9], np.float32), big.shape[1] // 3)[None, :] * np.tile(np.array([1.0, 1.0, 0.75], np.float32), big.shape[0] // 3)[:, None]
    h = big.shape[0]; c = np.float32([[0, 0], [h, 0], [h, h], [0, h]]); sz = 460 / 2
    pts = np.float32([[640 - sz, 360 - sz], [640 + sz, 360 - sz + 20], [640 + sz, 360 + sz], [640 - sz, 360 + sz - 20]])
    out = cv2.warpPerspective(big, cv2.getPerspectiveTransform(c, pts), (1280, 720), flags=cv2.INTER_LINEAR, borderValue=200)
    yy, xx = np.mgrid[0:720, 0:1280].astype(np.float32)
    light = 0.75 + 0.35 * np.exp(-(((xx - 900) / 380) ** 2 + ((yy - 220) / 260) ** 2)) - 0.12 * (((xx - 640) / 640) ** 2 + ((yy - 360) / 360) ** 2)
    out = cv2.GaussianBlur(out * light * 0.85 + 40, (0, 0), blur)
    out = np.clip(out + r.normal(0, 5, out.shape), 0, 255).astype(np.uint8)
    ok, j = cv2.imencode(".jpg", out, [cv2.IMWRITE_JPEG_QUALITY, 60])
    return cv2.imdecode(j, cv2.IMREAD_GRAYSCALE)


def camera_from_pictures(scenes, tag):
    """A fake camera playing these pictures, each for its number of seconds (12 camera pictures a second). The video is
    lossless, so SeedLock gets exactly these pictures (their webcam-like JPEG damage included) and nothing else."""
    shots = tempfile.mkdtemp(prefix=tag); i = 0
    for picture, seconds in scenes:
        for _ in range(int(seconds * 12)):
            cv2.imwrite(os.path.join(shots, f"f{i:04d}.png"), picture); i += 1
    video = os.path.join(shots, "camera.mkv")
    subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-framerate", "12", "-i", os.path.join(shots, "f%04d.png"), "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p", video], check=True)
    w = os.path.join(shots, "ffmpeg-as-camera")
    open(w, "w").write(f"""#!/usr/bin/env python3
import os, sys
real, video, args = {FFMPEG!r}, {video!r}, sys.argv[1:]
if "-version" in args:
    os.execv(real, [real, "-version"])
os.execv(real, [real, "-hide_banner", "-loglevel", "error", "-nostdin", "-re", "-stream_loop", "-1", "-i", video] + args[args.index("-an"):])
""")
    os.chmod(w, 0o755)
    return w


def register_from_camera(wrapper, keys, name, expect_hint=False):
    first = Descriptor.from_string(descsum_create(f"wsh(sortedmulti(2,{keys.replace('<0;1>', '0')}))")).derive(0).address(TEST)
    os.environ["SEEDLOCK_FFMPEG"] = wrapper
    s = Session()
    os.environ.pop("SEEDLOCK_FFMPEG")
    s.expect(r"\n> "); s.send("10")
    s.text("Choose 1, 2 or 3 (or press Enter to go back): "); s.send("2")
    s.text("or type c to scan Sparrow's QR code with the camera (q to go back): "); s.send("c")
    if expect_hint:
        s.text("A QR code is in view but cannot be read yet: hold the camera steady and closer")
    s.text("Read the wallet's QR code.")
    s.text("Multisig wallet: 2 of 2 signatures needed, Native Segwit (P2WSH), a test network (testnet or signet)")
    s.expect(r"Its first receive address: (tb1q\w+)\r\n")
    assert s.match.group(1) == first, (s.match.group(1), first)
    s.text("Does it match, and are these the right cosigners? [y/N] "); s.send("y")
    s.text("Name for this wallet [multisig-2-of-2]: "); s.send(name)
    s.text(f'Registered as "{name}":')
    s.expect(r"\n> "); s.send("q"); s.expect(pexpect.EOF)


# J1) UR, one static code (the wallet of bob and carol), a damaged copy first
pair = ("bob", "carol")
bc_keys = ",".join(f"[{fps[n]}/48h/1h/0h/2h]{tpubs[n]}/<0;1>/*" for n in pair)
bc_output = Output([SCRIPT_EXPRESSION_TAG_MAP[401], SCRIPT_EXPRESSION_TAG_MAP[407]],
                   MultiKey(2, [], [sparrow_hdkey(n, ACCOUNT, f"Keystore {i + 1}") for i, n in enumerate(pair)]))
enc = UREncoder(UR("crypto-output", bytearray(bc_output.to_cbor())), 400, 10)          # Sparrow's medium density
ur_static = enc.next_part().upper()
assert enc.is_single_part()
screen, ur_version = sparrow_screen(ur_static)
register_from_camera(camera_from_pictures([(screen_photo(damaged(screen, ur_version)), 6), (screen_photo(screen), 3)], "j1-"), bc_keys, "pair-bc", expect_hint=True)
done = [f"UR (version {ur_version})"]
# J2) BBQr, one static code (the wallet of alice and carol), photographed so that only SeedLock's newer tries read
#     it (checked against the reader without them when this test was written)
if opts.bbqr_lib:
    pair = ("alice", "carol")
    ac_keys = ",".join(f"[{fps[n]}/48h/1h/0h/2h]{tpubs[n]}/<0;1>/*" for n in pair)
    _, bb = split_qrs(descsum_create(f"wsh(sortedmulti(2,{ac_keys}))").encode(), "U", encoding="Z", max_split=1)
    assert len(bb) == 1
    screen, bb_version = sparrow_screen(bb[0])
    register_from_camera(camera_from_pictures([(screen_photo(screen, seed=J2_SEED, blur=J2_BLUR), 3)], "j2-"), ac_keys, "pair-ac")
    done.append(f"BBQr (version {bb_version})")
print(f"J) a whole wallet in one static QR code as Sparrow shows it at medium density ({', '.join(done)}), photographed on a "
      "screen (pixel pattern, glare, soft focus, JPEG): registered through the real ffmpeg; with a damaged copy in view, "
      "SeedLock said it sees a code it cannot read and what helps: OK")
print("\nALL MULTISIG CHECKS PASSED")
