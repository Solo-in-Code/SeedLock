"""The independent half of android/jvm-check: embit (a Python Bitcoin library) makes a seed and a
transaction for it, and a 2-of-3 multisig wallet with a payment of its own, and afterwards checks the
signatures that SeedLock's Kotlin path added, with its own sighash code, and the QR codes for Sparrow
(the wallet's, and a cosigner's key), read with urtypes (the library SeedSigner uses).

    python3 psbt_check.py make FOLDER     writes FOLDER/words.txt and FOLDER/in.psbt, and the multisig
                                          wallet: FOLDER/ms_words.txt, ms_descriptor.txt, ms_first.txt, ms.psbt
    python3 psbt_check.py verify FOLDER   checks FOLDER/signed.b64 and FOLDER/sparrow.txt, and
                                          FOLDER/ms_signed.b64 and FOLDER/cosigner.txt
"""
import os, sys, zlib
from embit import bip32, bip39, ec, script
from embit.descriptor import Descriptor
from embit.networks import NETWORKS
from embit.psbt import PSBT, DerivationPath
from embit.transaction import Transaction, TransactionInput, TransactionOutput
from mnemonic import Mnemonic

H = bip32.HARDENED_INDEX
IN_PATH, CHANGE_PATH = [84 + H, H, H, 0, 3], [84 + H, H, H, 1, 7]
MS_ACCOUNT = [48 + H, 1 + H, H, 2 + H]  # BIP 48, native SegWit multisig, on a test network
TEST = NETWORKS["test"]


def keys(words):
    root = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(words))
    return root.my_fingerprint, root.derive(IN_PATH).key.get_public_key(), root.derive(CHANGE_PATH).key.get_public_key()


def make(folder):
    words = Mnemonic("english").generate(256)
    fp, in_pub, change_pub = keys(words)
    outside = script.address_to_scriptpubkey("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")
    tx = Transaction(version=2, locktime=0,
                     vin=[TransactionInput(bytes.fromhex("aa" * 32), 1)],
                     vout=[TransactionOutput(60_000, outside), TransactionOutput(39_000, script.p2wpkh(change_pub))])
    psbt = PSBT(tx)
    psbt.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wpkh(in_pub))
    psbt.inputs[0].bip32_derivations[in_pub] = DerivationPath(fp, IN_PATH)
    psbt.outputs[1].bip32_derivations[change_pub] = DerivationPath(fp, CHANGE_PATH)
    open(os.path.join(folder, "words.txt"), "w").write(words)
    open(os.path.join(folder, "in.psbt"), "w").write(psbt.to_string())
    print("embit made a 24-word seed and a transaction for it (100 000 sat in: 60 000 out, 39 000 change)")
    make_multisig(folder)


def descsum_create(body):
    """A descriptor with its checksum, by the reference code of BIP 380."""
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
        v = charset.find(c)
        symbols.append(v & 31)
        groups.append(v >> 5)
        if len(groups) == 3:
            symbols.append(groups[0] * 9 + groups[1] * 3 + groups[2])
            groups = []
    if len(groups) == 1:
        symbols.append(groups[0])
    elif len(groups) == 2:
        symbols.append(groups[0] * 3 + groups[1])
    c = polymod(symbols + [0] * 8) ^ 1
    return body + "#" + "".join("qpzry9x8gf2tvdw0s3jn54khce6mua7l"[(c >> (5 * (7 - i))) & 31] for i in range(8))


def ms_roots(folder):
    words = open(os.path.join(folder, "ms_words.txt")).read().split("\n")[:3]
    return [bip32.HDKey.from_seed(bip39.mnemonic_to_seed(w)) for w in words]


def ms_script(roots, chain, index):
    """The 2-of-3 script at chain/index: the three keys sorted (BIP 67), as sortedmulti says."""
    keys = sorted((r.derive(MS_ACCOUNT + [chain, index]).key.get_public_key() for r in roots), key=lambda k: k.sec())
    return script.multisig(2, keys)


def make_multisig(folder):
    words = [Mnemonic("english").generate(128) for _ in range(3)]
    open(os.path.join(folder, "ms_words.txt"), "w").write("\n".join(words) + "\n")
    roots = ms_roots(folder)

    def descriptor(tail):
        keys = ",".join(f"[{r.my_fingerprint.hex()}/48h/1h/0h/2h]{r.derive(MS_ACCOUNT).to_public().to_base58(version=TEST['xpub'])}/{tail}" for r in roots)
        return descsum_create(f"wsh(sortedmulti(2,{keys}))")
    # as Sparrow exports it: File > Export Wallet > Output Descriptor
    open(os.path.join(folder, "ms_descriptor.txt"), "w").write(
        "# Receive and change descriptor:\n" + descriptor("<0;1>/*") + "\n\n\n# Receive descriptor:\n" + descriptor("0/*") +
        "\n\n# Change descriptor:\n" + descriptor("1/*") + "\n")
    first = Descriptor.from_string(descriptor("0/*")).derive(0).address(TEST)
    assert first == script.p2wsh(ms_script(roots, 0, 0)).address(TEST), "embit's descriptor engine and its scripts agree"
    open(os.path.join(folder, "ms_first.txt"), "w").write(first)
    outside = script.p2wpkh(ec.PrivateKey(bytes([9]) * 32).get_public_key())
    ws_in, ws_change = ms_script(roots, 0, 0), ms_script(roots, 1, 0)
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(bytes.fromhex("dd" * 32), 0)],
                     vout=[TransactionOutput(60_000, outside), TransactionOutput(39_000, script.p2wsh(ws_change))])
    psbt = PSBT(tx)
    psbt.inputs[0].witness_utxo = TransactionOutput(100_000, script.p2wsh(ws_in))
    psbt.inputs[0].witness_script = ws_in
    psbt.outputs[1].witness_script = ws_change
    for r in roots:
        psbt.inputs[0].bip32_derivations[r.derive(MS_ACCOUNT + [0, 0]).key.get_public_key()] = DerivationPath(r.my_fingerprint, MS_ACCOUNT + [0, 0])
        psbt.outputs[1].bip32_derivations[r.derive(MS_ACCOUNT + [1, 0]).key.get_public_key()] = DerivationPath(r.my_fingerprint, MS_ACCOUNT + [1, 0])
    open(os.path.join(folder, "ms.psbt"), "w").write(psbt.to_string())
    print("embit made a 2-of-3 multisig wallet (described as Sparrow exports it) and a payment from it")


def ur_payload(text):
    """The bytes of a single-part UR (BCR-2020-005: minimal bytewords, the first and last letter of each
    word, and a CRC32 at the end), decoded here from the specification."""
    words = ("ableacidalsoapexaquaarchatomauntawayaxisbackbaldbarnbeltbetabiasbluebodybragbrewbulbbuzzcalmcash"
             "catschefcityclawcodecolacookcostcruxcurlcuspcyandarkdatadaysdelidicedietdoordowndrawdropdrumdull"
             "dutyeacheasyechoedgeepicevenexamexiteyesfactfairfernfigsfilmfishfizzflapflewfluxfoxyfreefrogfuel"
             "fundgalagamegeargemsgiftgirlglowgoodgraygrimgurugushgyrohalfhanghardhawkheathelphighhillholyhope"
             "hornhutsicedideaidleinchinkyintoirisironitemjadejazzjoinjoltjowljudojugsjumpjunkjurykeepkenokept"
             "keyskickkilnkingkitekiwiknoblamblavalazyleaflegsliarlimplionlistlogoloudloveluaulucklungmainmany"
             "mathmazememomenumeowmildmintmissmonknailnavyneednewsnextnoonnotenumbobeyoboeomitonyxopenovalowls"
             "paidpartpeckplaypluspoempoolposepuffpumapurrquadquizraceramprealredorichroadrockroofrubyruinruns"
             "rustsafesagascarsetssilkskewslotsoapsolosongstubsurfswantacotasktaxitenttiedtimetinytoiltombtoys"
             "triptunatwinuglyundouniturgeuservastveryvetovialvibeviewvisavoidvowswallwandwarmwaspwavewaxywebs"
             "whatwhenwhizwolfworkyankyawnyellyogayurtzapszerozestzinczonezoom")
    index = {words[i * 4] + words[i * 4 + 3]: i for i in range(256)}
    body = text.lower().split("/", 1)[1]
    data = bytes(index[body[i:i + 2]] for i in range(0, len(body), 2))
    payload, crc = data[:-4], data[-4:]
    assert zlib.crc32(payload).to_bytes(4, "big") == crc, "the UR's checksum"
    return payload


def verify_sparrow(folder, words):
    """The wallet's QR code for Sparrow: a UR crypto-output that urtypes reads as embit's own account key."""
    from urtypes.crypto import Output
    text = open(os.path.join(folder, "sparrow.txt")).read().strip()
    assert text.startswith("UR:CRYPTO-OUTPUT/"), text[:30]
    output = Output.from_cbor(ur_payload(text))
    key = output.hd_key()
    root = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(words))
    account = root.derive([84 + H, H, H])
    assert [e.expression for e in output.script_expressions] == ["wpkh"], output.script_expressions
    assert key.key == account.key.get_public_key().sec() and key.chain_code == account.chain_code
    assert key.origin.source_fingerprint == root.my_fingerprint
    assert [(c.index, c.hardened) for c in key.origin.components] == [(84, True), (0, True), (0, True)]
    assert key.name == "embit check", key.name
    print("OK   urtypes reads the QR code for Sparrow as embit's own wpkh account key, named \"embit check\"")


def verify_multisig(folder):
    """Both cosigners' signatures, over embit's own BIP 143 signature hash (the witness script as script code),
    and the first cosigner's key as the QR code SeedSigner shows: what urtypes itself writes for that key."""
    from urtypes.crypto.account import Account
    from urtypes.crypto.hd_key import CoinInfo, HDKey
    from urtypes.crypto.keypath import Keypath, PathComponent
    from urtypes.crypto.output import SCRIPT_EXPRESSION_TAG_MAP, Output
    roots = ms_roots(folder)
    signed = PSBT.from_string(open(os.path.join(folder, "ms_signed.b64")).read().strip())
    ws_in = ms_script(roots, 0, 0)
    digest = signed.tx.sighash_segwit(0, ws_in, 100_000)
    sigs = signed.inputs[0].partial_sigs
    assert set(sigs) == {roots[i].derive(MS_ACCOUNT + [0, 0]).key.get_public_key() for i in (0, 1)}, list(sigs)
    for key, sig in sigs.items():
        assert sig[-1] == 1, "SIGHASH_ALL"
        assert key.verify(ec.Signature.parse(sig[:-1]), digest), "a signature does not verify in embit"
    print("OK   embit verifies both cosigners' signatures of the multisig payment (BIP 143, the witness script as script code)")

    text = open(os.path.join(folder, "cosigner.txt")).read().strip()
    assert text.startswith("UR:CRYPTO-ACCOUNT/"), text[:30]
    root = roots[0]
    key = root.derive(MS_ACCOUNT).to_public()
    origin = Keypath([PathComponent(i & ~H, bool(i & H)) for i in MS_ACCOUNT], root.my_fingerprint, len(MS_ACCOUNT))
    hdkey = HDKey({"key": key.key.sec(), "chain_code": key.chain_code, "use_info": CoinInfo(0, 1), "origin": origin,
                   "parent_fingerprint": key.fingerprint, "name": "alice"})
    expected = Account(root.my_fingerprint, [Output([SCRIPT_EXPRESSION_TAG_MAP[401]], hdkey)]).to_cbor()
    assert ur_payload(text) == expected, "the cosigner's QR code is not what urtypes writes for that key"
    print("OK   the cosigner's QR code is byte for byte the crypto-account urtypes writes for embit's key, named \"alice\"")


def verify(folder):
    verify_multisig(folder)
    words = open(os.path.join(folder, "words.txt")).read().strip()
    verify_sparrow(folder, words)
    _, in_pub, _ = keys(words)
    signed = PSBT.from_string(open(os.path.join(folder, "signed.b64")).read().strip())
    sig = signed.inputs[0].partial_sigs[in_pub]
    assert sig[-1] == 1, "SIGHASH_ALL"
    assert in_pub.verify(ec.Signature.parse(sig[:-1]), signed.sighash(0)), "the signature does not verify in embit"
    print("OK   embit verifies the signature made through Kotlin with its own sighash code")
    signed.inputs[0].witness_utxo = TransactionOutput(100_001, script.p2wpkh(in_pub))
    assert not in_pub.verify(ec.Signature.parse(sig[:-1]), signed.sighash(0))
    print("OK   and it no longer verifies once the amount is changed (the check itself works)")


if __name__ == "__main__":
    {"make": make, "verify": verify}[sys.argv[1]](sys.argv[2])
