"""The independent half of android/jvm-check: embit (a Python Bitcoin library) makes a seed and a
transaction for it, and a 2-of-3 multisig wallet with a payment of its own, and a payment to a silent payment
address as Sparrow sends it; afterwards it checks the signatures that SeedLock's Kotlin path added, with its
own sighash code, the QR codes for Sparrow (the wallet's, and a cosigner's key), read with urtypes (the library
SeedSigner uses), and the silent payment wallet and outputs, worked out here from BIP 352 with plain Python
arithmetic on the curve.

    python3 psbt_check.py make FOLDER     writes FOLDER/words.txt and FOLDER/in.psbt, the multisig wallet:
                                          FOLDER/ms_words.txt, ms_descriptor.txt, ms_first.txt, ms.psbt, and the
                                          silent payments: FOLDER/sp.psbt, sp_receiver.txt, sp_spend.psbt
    python3 psbt_check.py verify FOLDER   checks FOLDER/signed.b64 and FOLDER/sparrow.txt, FOLDER/ms_signed.b64
                                          and FOLDER/cosigner.txt, and FOLDER/sp_wallet.txt, sp_signed.b64 and
                                          sp_spend_signed.b64
"""
import base64, os, sys, zlib
from embit import bech32, bip32, bip39, ec, hashes, script
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
    make_silent(folder)


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


# --------------------------------------------------------------------------- silent payments (BIP 352, 374, 375)

CURVE_P = 2**256 - 2**32 - 977
CURVE_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
CURVE_G = (0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798,
           0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8)
SP_SCAN, SP_SPEND = [352 + H, H, H, 1 + H, 0], [352 + H, H, H, H, 0]
SP_TXID, SP_VOUT = bytes([0xcc]) * 32, 2


def add(a, b):
    """The sum of two points (None: the point at infinity), in plain Python: nothing shared with SeedLock's code."""
    if a is None or b is None:
        return a or b
    if a[0] == b[0] and (a[1] + b[1]) % CURVE_P == 0:
        return None
    if a == b:
        slope = 3 * a[0] * a[0] * pow(2 * a[1], CURVE_P - 2, CURVE_P)
    else:
        slope = (b[1] - a[1]) * pow(b[0] - a[0], CURVE_P - 2, CURVE_P)
    x = (slope * slope - a[0] - b[0]) % CURVE_P
    return x, (slope * (a[0] - x) - a[1]) % CURVE_P


def mul(k, point=CURVE_G):
    result = None
    for i in range(256):
        if (k >> i) & 1:
            result = add(result, point)
        point = add(point, point)
    return result


def sec(point):
    return bytes([2 + (point[1] & 1)]) + point[0].to_bytes(32, "big")


def point(data):
    x = int.from_bytes(data[1:], "big")
    y = pow((x ** 3 + 7) % CURVE_P, (CURVE_P + 1) // 4, CURVE_P)
    return x, (y if y % 2 == data[0] % 2 else CURVE_P - y)


def tagged(tag, *parts):
    return int.from_bytes(hashes.tagged_hash(tag, b"".join(parts)), "big") % CURVE_N


def found(b_scan, spend, a, txid=SP_TXID, vout=SP_VOUT, k=0):
    """The key of the output that pays (scan key, spend) with this k, as its receiver finds it (BIP 352): from the
    scan key's private key, the smallest outpoint (the only one here) and the input's public key."""
    outpoint = txid + vout.to_bytes(4, "little")
    shared = mul(tagged("BIP0352/Inputs", outpoint, sec(a)) * b_scan % CURVE_N, a)
    return add(spend, mul(tagged("BIP0352/SharedSecret", sec(shared), k.to_bytes(4, "big"))))[0].to_bytes(32, "big")


def dleq_verify(a, b, c, proof):
    """BIP 374's VerifyProof, with G as the generator and no message."""
    e, s = int.from_bytes(proof[:32], "big"), int.from_bytes(proof[32:], "big")
    if e >= CURVE_N or s >= CURVE_N:
        return False
    r1 = add(mul(s), mul(CURVE_N - e, a))
    r2 = add(mul(s, b), mul(CURVE_N - e, c))
    if r1 is None or r2 is None:
        return False
    return e == tagged("BIP0374/challenge", sec(a), sec(b), sec(c), sec(CURVE_G), sec(r1), sec(r2))


def own_silent_keys(words):
    """The wallet's silent payment keys, derived by embit: the scan private key (as a number) and the spend public
    key (as a point); and its change key, the spend key with label 0."""
    root = bip32.HDKey.from_seed(bip39.mnemonic_to_seed(words))
    b_scan = int.from_bytes(root.derive(SP_SCAN).key.secret, "big")
    spend = point(root.derive(SP_SPEND).key.get_public_key().sec())
    change = add(spend, mul(tagged("BIP0352/Label", b_scan.to_bytes(32, "big"), (0).to_bytes(4, "big"))))
    return root, b_scan, spend, change


def psbt_map(fields):
    out = b""
    for kind, data, value in fields:
        key = bytes([kind]) + data
        assert len(key) < 0xfd and len(value) < 0xfd
        out += bytes([len(key)]) + key + bytes([len(value)]) + value
    return out + b"\x00"


def make_silent(folder):
    """A payment to someone's silent payment address, as Sparrow sends it: a PSBT of version 2 whose outputs to
    silent payment addresses have no script yet (the signer works them out). The wallet's coin (100 000 sat)
    pays 60 000 sat to the address, and 39 000 sat back to the wallet's own silent payment change (label 0)."""
    words = open(os.path.join(folder, "words.txt")).read().strip()
    root, b_scan_own, _, change = own_silent_keys(words)
    in_key = root.derive(IN_PATH).key.get_public_key()
    receiver_scan, receiver_spend = (int.from_bytes(os.urandom(32), "big") % CURVE_N for _ in range(2))
    le32 = lambda n: n.to_bytes(4, "little")
    utxo = (100_000).to_bytes(8, "little") + bytes([22]) + script.p2wpkh(in_key).data
    origin = root.my_fingerprint + b"".join(le32(i) for i in IN_PATH)
    bytes_ = b"psbt\xff"
    bytes_ += psbt_map([(0x02, b"", le32(2)), (0x03, b"", le32(0)), (0x04, b"", b"\x01"), (0x05, b"", b"\x02"),
                        (0x06, b"", b"\x03"), (0xfb, b"", le32(2))])
    bytes_ += psbt_map([(0x01, b"", utxo), (0x06, in_key.sec(), origin), (0x0e, b"", SP_TXID), (0x0f, b"", le32(SP_VOUT)),
                        (0x10, b"", le32(0xfffffffd))])
    bytes_ += psbt_map([(0x03, b"", (60_000).to_bytes(8, "little")), (0x09, b"", sec(mul(receiver_scan)) + sec(mul(receiver_spend)))])
    bytes_ += psbt_map([(0x03, b"", (39_000).to_bytes(8, "little")), (0x09, b"", sec(mul(b_scan_own)) + sec(change)), (0x0a, b"", le32(0))])
    open(os.path.join(folder, "sp.psbt"), "wb").write(bytes_)
    open(os.path.join(folder, "sp_receiver.txt"), "w").write(f"{receiver_scan:064x}\n{receiver_spend:064x}\n")
    print("made a payment to a silent payment address (version 2, as Sparrow sends it), with silent payment change")
    make_silent_spend(folder)


SPEND_TXID, SPEND_VOUT = bytes([0xee]) * 32, 0
OUTSIDE = script.p2wpkh(ec.PrivateKey(bytes([9]) * 32).get_public_key())


def received_output(words, tweak):
    """The output key of a silent payment the wallet received with this tweak (BIP 352): its spend key plus the
    tweak times G."""
    _, _, spend, _ = own_silent_keys(words)
    return add(spend, mul(tweak))


def make_silent_spend(folder):
    """Spending a silent payment the wallet received, as Sparrow sends it (BIP 376): the input's tweak and the
    spend key's path; 60 000 sat to someone else, 39 000 sat back to the wallet's own silent payment change."""
    words = open(os.path.join(folder, "words.txt")).read().strip()
    root, b_scan_own, spend_own, change = own_silent_keys(words)
    tweak = int.from_bytes(os.urandom(32), "big") % CURVE_N
    output = received_output(words, tweak)
    le32 = lambda n: n.to_bytes(4, "little")
    utxo = (100_000).to_bytes(8, "little") + bytes([34]) + b"\x51\x20" + output[0].to_bytes(32, "big")
    origin = root.my_fingerprint + b"".join(le32(i) for i in SP_SPEND)
    bytes_ = b"psbt\xff"
    bytes_ += psbt_map([(0x02, b"", le32(2)), (0x03, b"", le32(0)), (0x04, b"", b"\x01"), (0x05, b"", b"\x02"),
                        (0x06, b"", b"\x03"), (0xfb, b"", le32(2))])
    bytes_ += psbt_map([(0x01, b"", utxo), (0x0e, b"", SPEND_TXID), (0x0f, b"", le32(SPEND_VOUT)), (0x10, b"", le32(0xfffffffd)),
                        (0x1f, sec(spend_own), origin), (0x20, b"", tweak.to_bytes(32, "big"))])
    bytes_ += psbt_map([(0x03, b"", (60_000).to_bytes(8, "little")), (0x04, b"", OUTSIDE.data)])
    bytes_ += psbt_map([(0x03, b"", (39_000).to_bytes(8, "little")), (0x09, b"", sec(mul(b_scan_own)) + sec(change)), (0x0a, b"", le32(0))])
    open(os.path.join(folder, "sp_spend.psbt"), "wb").write(bytes_)
    open(os.path.join(folder, "sp_spend_tweak.txt"), "w").write(f"{tweak:064x}\n")
    print("made a transaction that spends a silent payment the wallet received (version 2, as Sparrow sends it)")


def parse_v2(data):
    """A PSBT of version 2 as its maps: {key: value} for the global one, then each input's and output's."""
    assert data[:5] == b"psbt\xff"
    at = 5

    def read_map():
        nonlocal at
        fields = {}
        while data[at] != 0:
            key = data[at + 1:at + 1 + data[at]]
            at += 1 + data[at]
            assert data[at] < 0xfd
            fields[key] = data[at + 1:at + 1 + data[at]]
            at += 1 + data[at]
        at += 1
        return fields
    glob = read_map()
    inputs = [read_map() for _ in range(glob[b"\x04"][0])]
    outputs = [read_map() for _ in range(glob[b"\x05"][0])]
    assert at == len(data)
    return glob, inputs, outputs


def verify_silent(folder):
    words = open(os.path.join(folder, "words.txt")).read().strip()
    root, b_scan_own, spend_own, change = own_silent_keys(words)
    scan_own = mul(b_scan_own)

    # the silent payment wallet for Sparrow, as embit's keys and BIP 352, 392 and 380 give it
    address, descriptor = open(os.path.join(folder, "sp_wallet.txt")).read().split("\n")[:2]
    v0 = lambda hrp, payload: bech32.bech32_encode(bech32.Encoding.BECH32M, hrp, [0] + bech32.convertbits(payload, 8, 5))
    assert address == v0("sp", sec(scan_own) + sec(spend_own)), address
    spscan = v0("spscan", b_scan_own.to_bytes(32, "big") + sec(spend_own))
    assert descriptor == descsum_create(f"sp([{root.my_fingerprint.hex()}/352h/0h/0h]{spscan})"), descriptor
    print("OK   the silent payment address and the sp() descriptor for Sparrow are what embit's keys give (BIP 352, 392)")

    # the signed payment: version 2, nothing modifiable any more, its outputs found by their receivers
    glob, inputs, outputs = parse_v2(base64.b64decode(open(os.path.join(folder, "sp_signed.b64")).read().strip()))
    assert glob[b"\xfb"] == (2).to_bytes(4, "little") and glob[b"\x06"] == b"\x00", glob
    in_private = root.derive(IN_PATH).key
    a = point(in_private.get_public_key().sec())
    receiver_scan, receiver_spend = (int(line, 16) for line in open(os.path.join(folder, "sp_receiver.txt")).read().split())
    assert outputs[0][b"\x04"] == b"\x51\x20" + found(receiver_scan, mul(receiver_spend), a), "the receiver does not find its payment"
    assert outputs[1][b"\x04"] == b"\x51\x20" + found(b_scan_own, change, a), "the wallet does not find its change"
    print("OK   the receiver finds its payment with its scan key, and the wallet its change (BIP 352, in plain Python)")
    # one share for each scan key, the input's key times it, with a proof that checks out
    shares = {k[1:]: v for k, v in glob.items() if k[0] == 0x07}
    proofs = {k[1:]: v for k, v in glob.items() if k[0] == 0x08}
    assert set(shares) == {sec(mul(receiver_scan)), sec(scan_own)} == set(proofs), list(shares)
    secret = int.from_bytes(in_private.secret, "big")
    for scan_key, share in shares.items():
        assert share == sec(mul(secret, point(scan_key))), "a share is not the input's key times the scan key"
        assert dleq_verify(a, point(scan_key), point(share), proofs[scan_key]), "a DLEQ proof does not verify"
    print("OK   each ECDH share is the input's key times the scan key, and its DLEQ proof verifies (BIP 374, 375)")
    # the signature, over embit's own signature hash of the transaction with those outputs
    in_pub = in_private.get_public_key()
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(SP_TXID, SP_VOUT, sequence=0xfffffffd)],
                     vout=[TransactionOutput(60_000, script.Script(outputs[0][b"\x04"])),
                           TransactionOutput(39_000, script.Script(outputs[1][b"\x04"]))])
    sig = inputs[0][b"\x02" + in_pub.sec()]
    assert sig[-1] == 1, "SIGHASH_ALL"
    assert in_pub.verify(ec.Signature.parse(sig[:-1]), tx.sighash_segwit(0, script.p2pkh(in_pub), 100_000))
    print("OK   embit verifies the signature over the transaction with the outputs SeedLock worked out")

    # spending a silent payment received: a BIP 340 signature by the output's key, over embit's own taproot
    # signature hash, and the change found by the wallet with that key as the input's
    tweak = int(open(os.path.join(folder, "sp_spend_tweak.txt")).read().strip(), 16)
    output = received_output(words, tweak)
    glob, inputs, outputs = parse_v2(base64.b64decode(open(os.path.join(folder, "sp_spend_signed.b64")).read().strip()))
    assert glob[b"\x06"] == b"\x00" and outputs[0][b"\x04"] == OUTSIDE.data, glob
    lifted = (output[0], output[1] if output[1] % 2 == 0 else CURVE_P - output[1])
    assert outputs[1][b"\x04"] == b"\x51\x20" + found(b_scan_own, change, lifted, SPEND_TXID, SPEND_VOUT), "the wallet does not find its change"
    spent = script.Script(b"\x51\x20" + output[0].to_bytes(32, "big"))
    tx = Transaction(version=2, locktime=0, vin=[TransactionInput(SPEND_TXID, SPEND_VOUT, sequence=0xfffffffd)],
                     vout=[TransactionOutput(60_000, OUTSIDE), TransactionOutput(39_000, script.Script(outputs[1][b"\x04"]))])
    signature = inputs[0][b"\x13"]
    assert len(signature) == 64, "SIGHASH_DEFAULT"
    key = ec.PublicKey.from_xonly(output[0].to_bytes(32, "big"))
    assert key.schnorr_verify(ec.SchnorrSig.parse(signature), tx.sighash_taproot(0, [spent], [100_000]))
    print("OK   spending a silent payment received: embit verifies the BIP 340 signature by the output's key (BIP 376)")


def verify(folder):
    verify_silent(folder)
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
