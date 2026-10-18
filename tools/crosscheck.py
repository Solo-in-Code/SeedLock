#!/usr/bin/env python3
"""
Independent cross-check of SeedLock's wallet layer against other Bitcoin software.

Don't just trust this project: this script feeds random seeds through the real
`seedlock` binary and compares every result with two independent libraries
(bip-utils and embit): master fingerprint, account xpub, key origin, descriptors
(checksum re-computed from the BIP380 specification, and re-imported by embit)
and receive/change addresses for all four address types, mainnet and testnet,
with and without a BIP39 passphrase.

    pip install bip-utils embit mnemonic
    python3 crosscheck.py --bin ../target/release/seedlock
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile

from bip_utils import (Base58Decoder, Bip32Slip10Secp256k1, Bip39SeedGenerator, Bip44, Bip44Changes,
                       Bip44Coins, Bip49, Bip49Coins, Bip84, Bip84Coins, Bip86, Bip86Coins)
from mnemonic import Mnemonic

try:
    from embit.descriptor import Descriptor
    from embit.networks import NETWORKS
    HAVE_EMBIT = True
except Exception:  # embit is optional
    HAVE_EMBIT = False

# ---- BIP380 descriptor checksum, straight from the specification ---------------------
INPUT_CHARSET = "0123456789()[],'/*abcdefgh@:$%{}IJKLMNOPQRSTUVWXYZ&+-.;<=>?!^_|~ijklmnopqrstuvwxyzABCDEFGH`#\"\\ "
CHECKSUM_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
GENERATOR = [0xF5DEE51989, 0xA9FDCA3312, 0x1BAB10E32D, 0x3706B1677A, 0x644D626FFD]


def descsum_polymod(symbols):
    chk = 1
    for value in symbols:
        top = chk >> 35
        chk = (chk & 0x7FFFFFFFF) << 5 ^ value
        for i in range(5):
            chk ^= GENERATOR[i] if ((top >> i) & 1) else 0
    return chk


def descsum_create(s):
    groups, symbols = [], []
    for c in s:
        v = INPUT_CHARSET.find(c)
        symbols.append(v & 31)
        groups.append(v >> 5)
        if len(groups) == 3:
            symbols.append(groups[0] * 9 + groups[1] * 3 + groups[2])
            groups = []
    if len(groups) == 1:
        symbols.append(groups[0])
    elif len(groups) == 2:
        symbols.append(groups[0] * 3 + groups[1])
    checksum = descsum_polymod(symbols + [0] * 8) ^ 1
    return "".join(CHECKSUM_CHARSET[(checksum >> (5 * (7 - i))) & 31] for i in range(8))


# ---- what the other libraries say -----------------------------------------------------
LIBS = {  # script -> (BIP class, coins class, purpose)
    "wpkh": (Bip84, Bip84Coins, 84),
    "tr": (Bip86, Bip86Coins, 86),
    "sh-wpkh": (Bip49, Bip49Coins, 49),
    "pkh": (Bip44, Bip44Coins, 44),
}
NETS = {"mainnet": ("BITCOIN", 0, "main"), "testnet": ("BITCOIN_TESTNET", 1, "test")}
WRAP = {"wpkh": "wpkh({})", "tr": "tr({})", "sh-wpkh": "sh(wpkh({}))", "pkh": "pkh({})"}


def expected(mnemonic, passphrase, script, net, change, count):
    bip, coins, purpose = LIBS[script]
    coin_name, coin_type, _ = NETS[net]
    seed = Bip39SeedGenerator(mnemonic).Generate(passphrase)
    master = Bip32Slip10Secp256k1.FromSeed(seed)
    xpub = master.DerivePath(f"m/{purpose}'/{coin_type}'/0'").PublicKey().ToExtended()
    acct = bip.FromSeed(seed, getattr(coins, coin_name)).Purpose().Coin().Account(0)
    chain = acct.Change(Bip44Changes.CHAIN_INT if change else Bip44Changes.CHAIN_EXT)
    addrs = [chain.AddressIndex(i).PublicKey().ToAddress() for i in range(count)]
    return master.FingerPrint().ToHex(), xpub, addrs


def run(binary, args, stdin):
    r = subprocess.run([binary] + args, input=stdin, text=True, capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(f"seedlock {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout


def check_one(binary, backup, pw, mnemonic, passphrase, script, net, change, count=3):
    args = ["info", backup, "--script", script, "--network", net, "--count", str(count)]
    stdin = pw + "\n"
    if passphrase:
        args.append("--passphrase")
        stdin += passphrase + "\n"
    if change:
        args.append("--change")
    out = run(binary, args, stdin)

    fp, xpub, addrs = expected(mnemonic, passphrase, script, net, change, count)
    problems = []

    got_fp = re.search(r"Wallet fingerprint : (\w{8})", out).group(1)
    if got_fp != fp:
        problems.append(f"fingerprint {got_fp} != {fp}")

    origin, got_xpub = re.search(r"^\s+\[(\w{8}/[0-9h/]+)\](\w+)$", out, re.M).groups()
    _, coin_type, _ = NETS[net]
    if origin != f"{fp}/{LIBS[script][2]}h/{coin_type}h/0h":
        problems.append(f"key origin {origin}")
    if net == "mainnet":
        if got_xpub != xpub:
            problems.append(f"xpub {got_xpub} != {xpub}")
    else:  # tpub differs from xpub only in the 4 version bytes
        a, b = Base58Decoder.CheckDecode(got_xpub), Base58Decoder.CheckDecode(xpub)
        if not got_xpub.startswith("tpub") or a[4:] != b[4:] or a[:4].hex() != "043587cf":
            problems.append("tpub differs from the independently derived key")

    for kind in ("receive", "change"):
        desc = re.search(rf"{kind}\s*: (\S+)", out).group(1)
        body, _, checksum = desc.partition("#")
        if checksum != descsum_create(body):
            problems.append(f"{kind} descriptor checksum")
        want_body = WRAP[script].format(f"[{origin}]{got_xpub}/{1 if kind == 'change' else 0}/*")
        if body != want_body:
            problems.append(f"{kind} descriptor body {body}")
        if HAVE_EMBIT:
            try:
                d = Descriptor.from_string(desc)  # embit verifies the checksum itself
                for i, want in enumerate(addrs if (kind == "change") == change else []):
                    got = str(d.derive(i).address(NETWORKS[NETS[net][2]]))
                    if got != want:
                        problems.append(f"embit {kind}/{i}: {got} != {want}")
            except Exception as e:  # e.g. an embit build without taproot descriptors
                if script != "tr":
                    problems.append(f"embit could not import the {kind} descriptor: {e}")

    listed = re.findall(r"^\s+m/[\d'/]+\s+(\S+)$", out, re.M)
    if listed != addrs:
        problems.append(f"addresses {listed} != {addrs}")
    return problems, len(addrs)


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


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", default="seedlock", help="path to the seedlock binary")
    ap.add_argument("--seeds", type=int, default=6, help="number of random seeds to test")
    a = ap.parse_args()
    if os.path.exists(a.bin):
        a.bin = os.path.abspath(a.bin)
    offline_stand_in(a.bin)

    work = tempfile.mkdtemp()
    os.chdir(work)
    pw = "Correct-Horse-Battery-Staple"
    cases = [("abandon " * 11 + "about", "")]  # the standard test seed first
    mn = Mnemonic("english")
    passphrases = ["", "TREZOR", "pässphräse-é", "cafe\u0301 with spaces"]
    for i in range(a.seeds):
        cases.append((mn.generate(256 if i % 2 else 128), passphrases[i % len(passphrases)]))

    checked = failures = 0
    for n, (mnemonic, passphrase) in enumerate(cases):
        backup = f"seed{n}.seedlock"
        run(a.bin, ["encrypt", "--out", backup, "--kdf", "light"], f"{mnemonic}\n{pw}\n")
        for script in LIBS:
            for net, change in (("mainnet", False), ("mainnet", True), ("testnet", False)):
                if net == "testnet" and script in ("pkh", "sh-wpkh") and n % 2:
                    continue  # keep the run short; every script is still covered on testnet
                problems, cnt = check_one(a.bin, backup, pw, mnemonic, passphrase, script, net, change)
                checked += cnt
                if problems:
                    failures += 1
                    print(f"MISMATCH seed#{n} passphrase={passphrase!r} {script} {net} change={change}:", *problems, sep="\n   ")
        print(f"seed #{n} ({len(mnemonic.split())} words, passphrase {'yes' if passphrase else 'no'}): done")

    print(f"\n{checked} addresses compared, plus fingerprints, xpubs, origins and descriptors: "
          f"{'ALL MATCH' if not failures else str(failures) + ' MISMATCHES'}"
          f"{'' if HAVE_EMBIT else '  (embit not installed: descriptor re-import skipped)'}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
