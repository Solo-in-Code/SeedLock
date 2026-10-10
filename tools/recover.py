#!/usr/bin/env python3
"""
Recover a SeedLock backup WITHOUT the SeedLock program.

This is an independent, ~70-line implementation of docs/FORMAT.md. If the Rust
tool ever becomes unavailable, this script (plus the format document) is all
you need to get your mnemonic back from a .seedlock file and its password.

    pip install argon2-cffi pynacl mnemonic
    python3 recover.py my-wallet.seedlock            # asks for the password
    python3 recover.py --selftest                    # check the tool chain against a known vector

Run it on an offline computer: it prints your mnemonic.
"""
import base64
import getpass
import hashlib
import sys
import unicodedata

from argon2.low_level import Type, hash_secret_raw
from nacl.bindings import crypto_aead_xchacha20poly1305_ietf_decrypt
from nacl.exceptions import CryptoError

PREFIX = "SEEDLOCK1:"


def parse_backup(text):
    """Text form -> binary container. Tolerates comments, whitespace, dashes, lower case."""
    lines = [l.strip() for l in text.splitlines() if l.strip() and not l.strip().startswith("#")]
    body = "".join("".join(lines).split()).replace("-", "").upper()
    if not body.startswith(PREFIX):
        sys.exit("This is not a SeedLock backup (missing the SEEDLOCK1: prefix).")
    body = body[len(PREFIX):]
    data = base64.b32decode(body + "=" * (-len(body) % 8))
    container, checksum = data[:-4], data[-4:]
    if hashlib.sha256(container).digest()[:4] != checksum:
        sys.exit("The backup text is damaged or was copied incorrectly (checksum mismatch).")
    return container


def decrypt(container, password):
    """Binary container + password -> entropy bytes."""
    if container[:3] != b"SLK" or container[3] != 1 or container[4] != 1 or len(container) != 100:
        sys.exit("Unsupported backup format (expected SLK, version 1, Argon2id).")
    m_cost_kib = int.from_bytes(container[5:9], "little")
    t_cost, parallelism = container[9], container[10]
    salt, nonce = container[11:27], container[27:51]
    header, ciphertext = container[:51], container[51:]
    key = hash_secret_raw(
        unicodedata.normalize("NFKD", password).encode("utf-8"),
        salt,
        time_cost=t_cost,
        memory_cost=m_cost_kib,
        parallelism=parallelism,
        hash_len=32,
        type=Type.ID,
        version=19,
    )
    try:
        payload = crypto_aead_xchacha20poly1305_ietf_decrypt(ciphertext, header, nonce, key)
    except CryptoError:
        sys.exit("Wrong password, or the backup has been modified.")
    n = payload[0]
    if n not in (16, 20, 24, 28, 32) or any(payload[1 + n:]):
        sys.exit("Decrypted data has an unexpected layout.")
    return payload[1:1 + n]


def to_words(entropy):
    from mnemonic import Mnemonic  # only needed to turn entropy into words

    return Mnemonic("english").to_mnemonic(entropy)


# Known-answer vector, identical to the one in the Rust test-suite and docs/FORMAT.md.
SELFTEST_TEXT = (
    "SEEDLOCK1:KNGEWAIBAAAQAAABAEIBCEQTCQKRMFYYDENBWHA5DYPSAIJCEMSCKJRHFAUSUKZMFUXC6MBRGIZTINJWG74EM5MYIBOQAXY5PNR2F3S4BVA3F2QPGIKPZGVH2UVQ57VYCGYH6BB7JYQLXMSL5Q2OQKODKWGCWERO6WKGI5Y"
)


def main(argv):
    if "--selftest" in argv:
        entropy = decrypt(parse_backup(SELFTEST_TEXT), "correct-horse-battery-staple")
        assert entropy == bytes(range(16)), "self-test failed"
        print("Self-test passed: this tool chain decrypts the reference vector correctly.")
        return
    args = [a for a in argv if not a.startswith("--")]
    if len(args) != 1:
        sys.exit(__doc__)
    text = args[0] if args[0].upper().startswith(PREFIX) else open(args[0], encoding="utf-8").read()
    if "--password-stdin" in argv:
        password = sys.stdin.readline().rstrip("\r\n")
    else:
        password = getpass.getpass("Backup password: ")
    entropy = decrypt(parse_backup(text), password)
    print("Entropy (hex):", entropy.hex())
    print("Mnemonic     :", to_words(entropy))


if __name__ == "__main__":
    main(sys.argv[1:])
