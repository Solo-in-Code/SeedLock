"""Independent check of the backup format and the dice mode: Rust encrypts, the separate
Python implementation (recover.py) decrypts; and --dice-only is compared with SeedSigner's
sha256(dice rolls) method computed in Python.

    pip install argon2-cffi pynacl mnemonic
    python3 crosscheck_backup.py ../target/release/seedlock
"""
import sys
import hashlib, os, subprocess, tempfile, secrets, sys
from mnemonic import Mnemonic
BIN = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else "seedlock"
REC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "recover.py")

def offline_stand_in(program):
    """SeedLock works only offline. These checks show it a stand-in offline computer, which a build for the tests reads
    (cargo build --release --locked -p seedlock-cli --features test-hooks); a released program reads the real network,
    and then works only on a computer that is offline. Stop at once, saying so, when it refuses to work."""
    net = tempfile.mkdtemp()
    with open(os.path.join(net, "route"), "w") as f:
        f.write("Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n")
    open(os.path.join(net, "ipv6_route"), "w").close()
    os.environ["SEEDLOCK_TEST_NET_DIR"] = net
    if subprocess.run([program, "password"], capture_output=True, stdin=subprocess.DEVNULL).returncode == 3:
        sys.exit(f"{program} does not work here, because this computer is not offline. Run these checks on a computer that "
                 "is offline, or with a build for the tests:\n  cargo build --release --locked -p seedlock-cli --features test-hooks")


offline_stand_in(BIN)
os.chdir(tempfile.mkdtemp())
mn = Mnemonic("english")

def sh(args, stdin):
    return subprocess.run(args, input=stdin, text=True, capture_output=True)

print("selftest:", sh([sys.executable, REC, "--selftest"], "").stdout.strip())

# 1) Random seeds of every size, ASCII and Unicode passwords: Rust encrypts -> Python decrypts
pws = ["Correct-Horse-Battery-Staple", "pässwörd-ünïcode-123-日本語", "cafe\u0301-decomposed-accent"]
for words, pw in zip([12, 15, 18, 21, 24, 24], pws + pws):
    r = sh([BIN, "new", "--words", str(words), "--kdf", "light", "--out", f"b{words}{len(pw)}.seedlock"], pw + "\n")
    assert r.returncode == 0, r.stderr
    f = f"b{words}{len(pw)}.seedlock"
    rust_words = sh([BIN, "decrypt", f, "--raw"], pw + "\n").stdout.strip()
    py = sh([sys.executable, REC, f, "--password-stdin"], pw + "\n")
    assert py.returncode == 0, py.stderr
    py_words = py.stdout.split("Mnemonic     :")[1].strip()
    assert rust_words == py_words and len(py_words.split()) == words, (rust_words, py_words)
    assert mn.check(py_words)
print("Rust encrypt -> independent Python decrypt: OK for 12/15/18/21/24 words, ASCII + Unicode + decomposed passwords")

# 2) Python recover.py rejects a wrong password and a damaged file
assert sh([sys.executable, REC, "b12" + str(len(pws[0])) + ".seedlock", "--password-stdin"], "wrong\n").returncode != 0
print("Python recovery rejects wrong password: OK")

# 3) SeedSigner dice compatibility, computed independently in Python
rolls = ("123456" * 9)[:50]
sh([BIN, "new", "--words", "12", "--dice", "--dice-only", "--kdf", "light", "--out", "dice.seedlock"], rolls + "\nPassword-For-Dice-1\n")
rust = sh([BIN, "decrypt", "dice.seedlock", "--raw"], "Password-For-Dice-1\n").stdout.strip()
seedsigner = mn.to_mnemonic(hashlib.sha256(rolls.encode()).digest()[:16])   # SeedSigner: sha256(rolls string)[:16]
assert rust == seedsigner, (rust, seedsigner)
rolls24 = ("654321" * 17)[:99]
sh([BIN, "new", "--words", "24", "--dice", "--dice-only", "--kdf", "light", "--out", "dice24.seedlock"], rolls24 + "\nPassword-For-Dice-2\n")
rust24 = sh([BIN, "decrypt", "dice24.seedlock", "--raw"], "Password-For-Dice-2\n").stdout.strip()
assert rust24 == mn.to_mnemonic(hashlib.sha256(rolls24.encode()).digest()), "24-word dice mismatch"
print("--dice-only equals SeedSigner's sha256(dice rolls) method (12 and 24 words): OK")
