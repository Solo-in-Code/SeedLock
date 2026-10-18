"""Independent check of PSBT signing: embit builds a PSBT, seedlock signs it, and embit
verifies the signature with its own sighash implementation.

    pip install embit mnemonic
    python3 crosscheck_psbt.py ../target/release/seedlock
"""
import base64, os, subprocess, sys, tempfile
from embit import bip32, bip39, script, ec
from embit.descriptor import Descriptor
from embit.networks import NETWORKS
from embit.psbt import PSBT, DerivationPath
from embit.transaction import Transaction, TransactionInput, TransactionOutput
from mnemonic import Mnemonic

BIN = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else "seedlock"

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
H = bip32.HARDENED_INDEX

# does embit really import taproot descriptors? (so the crosscheck did not silently skip them)
try:
    d = Descriptor.from_string("tr([73c5da0a/86h/0h/0h]xpub6BgBgsespWvERF3LHQu6CnqdvfEvtMcQjYrcRzx53QJjSxarj2afYWcLteoGVky7D3UKDP9QyrLprQ3VCECoY49yfdDEHGCtMMj92pReUsQ/0/*)")
    print("embit taproot descriptor import:", d.derive(0).address(NETWORKS["main"]))
except Exception as e:
    print("embit taproot import unsupported:", e)

mn = Mnemonic("english").generate(256)
pw = "Correct-Horse-Battery-Staple"
subprocess.run([BIN, "encrypt", "--out", "w.seedlock", "--kdf", "light"], input=f"{mn}\n{pw}\n", text=True, capture_output=True, check=True)

root = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(mn))
fp = root.my_fingerprint
in_path, ch_path = [84 + H, H, H, 0, 3], [84 + H, H, H, 1, 7]
in_pub = root.derive(in_path).key.get_public_key()
ch_pub = root.derive(ch_path).key.get_public_key()
outside = script.address_to_scriptpubkey("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")

tx = Transaction(version=2, locktime=0,
                 vin=[TransactionInput(bytes.fromhex("aa" * 32), 1)],
                 vout=[TransactionOutput(60_000, outside), TransactionOutput(39_000, script.p2wpkh(ch_pub))])
psbt = PSBT(tx)
psbt.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wpkh(in_pub))
psbt.inputs[0].bip32_derivations[in_pub] = DerivationPath(fp, in_path)
psbt.outputs[1].bip32_derivations[ch_pub] = DerivationPath(fp, ch_path)
open("in.psbt", "w").write(psbt.to_string())

r = subprocess.run([BIN, "sign", "w.seedlock", "--psbt", "in.psbt", "--yes"], input=pw + "\n", text=True, capture_output=True)
assert r.returncode == 0, r.stderr
assert "change back to you" in r.stderr and "0.00001000 BTC" in r.stderr, r.stderr
signed = PSBT.from_string(r.stdout.strip())

sig = signed.inputs[0].partial_sigs[in_pub]                 # DER signature + sighash byte
assert sig[-1] == 1                                          # SIGHASH_ALL
sighash = signed.sighash(0)                                  # embit's own BIP143 sighash computation
assert in_pub.verify(ec.Signature.parse(sig[:-1]), sighash), "signature does not verify in embit"
print("PSBT built by embit -> signed by seedlock -> signature verified by embit's independent sighash: OK")

# a tampered amount must invalidate the signature (sanity check of the check itself)
signed.inputs[0].witness_utxo = TransactionOutput(100_001, script.p2wpkh(in_pub))
assert not in_pub.verify(ec.Signature.parse(sig[:-1]), signed.sighash(0))
print("sanity: signature is bound to the amount: OK")
