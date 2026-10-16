# SeedLock backup format, version 1

This document is the complete specification of a `.seedlock` backup. It is
written so that anyone can re-implement decryption in any language, in ten
years, without SeedLock. A ~70-line reference implementation is in
[`tools/recover.py`](../tools/recover.py).

## 1. Goals

* Safe to store anywhere (cloud, USB stick, e-mail): useless without the password.
* Any change to the file is detected (wrong password and tampering are indistinguishable by design).
* Self-describing: the key-derivation cost is stored inside the file, so old
  backups keep opening even if the defaults change later.
* All backups have the same size, whether they hold 12 or 24 words.
* Small enough for one QR code.

## 2. Binary container (100 bytes)

All multi-byte integers are little-endian.

| Offset | Size | Field                | Value / meaning                                            |
|-------:|-----:|----------------------|------------------------------------------------------------|
|      0 |    3 | `magic`              | ASCII `SLK`                                                |
|      3 |    1 | `version`            | `1`                                                        |
|      4 |    1 | `kdf`                | `1` = Argon2id, version 0x13                               |
|      5 |    4 | `m_cost`             | Argon2 memory in KiB (u32)                                 |
|      9 |    1 | `t_cost`             | Argon2 passes (u8)                                         |
|     10 |    1 | `parallelism`        | Argon2 lanes (u8)                                          |
|     11 |   16 | `salt`               | random, unique per backup                                  |
|     27 |   24 | `nonce`              | random, unique per backup (XChaCha20 nonce)                |
|     51 |   49 | `ciphertext`         | 33 bytes of encrypted payload + 16-byte Poly1305 tag       |

Bytes `0..51` are the **header**. It is authenticated as AEAD associated data,
so the KDF settings, salt and nonce cannot be altered without detection.

## 3. Key derivation

```
key = Argon2id( password = UTF-8( NFKD(password) ),
                salt     = container[11..27],
                m = m_cost KiB, t = t_cost, p = parallelism,
                version = 0x13, output = 32 bytes, no secret, no associated data )
```

The password is Unicode-normalised to **NFKD** (the same rule BIP39 uses for
passphrases) so that the same password typed on macOS, Windows, Linux or a phone
keyboard yields the same key. No trimming or case folding is applied.

Default presets written by SeedLock (`t = 3`, `p = 4` for all):

| Preset     | m_cost (KiB) | Memory  |
|------------|-------------:|--------:|
| `light`    |       65 536 |  64 MiB |
| `standard` |      262 144 | 256 MiB |
| `strong`   |    1 048 576 |   1 GiB |

Readers must reject values outside these bounds before allocating memory, so a
hostile file cannot exhaust the machine: `1 <= p <= 16`, `1 <= t <= 32`,
`8*p <= m_cost <= 2 097 152` (2 GiB).

## 4. Encryption

`XChaCha20-Poly1305` (24-byte nonce, 16-byte tag), key from section 3,
nonce = `container[27..51]`, associated data = `container[0..51]`.

## 5. Plaintext payload (33 bytes)

```
payload[0]        n   = entropy length in bytes: 16, 20, 24, 28 or 32
payload[1..1+n]       = BIP39 entropy (no checksum bits)
payload[1+n..33]      = zero bytes (padding)
```

The mnemonic is the standard BIP39 encoding of that entropy with the **English**
word list (12, 15, 18, 21 or 24 words for n = 16, 20, 24, 28, 32). Readers must
reject a payload whose padding is not all zero.

The BIP39 passphrase ("25th word") is deliberately **not** stored.

## 6. Text form

```
"SEEDLOCK1:" + BASE32( container || SHA256(container)[0..4] )
```

* BASE32 is RFC 4648, upper case, **no** `=` padding. Upper case lets QR codes
  use their compact alphanumeric mode.
* The 4-byte checksum catches typos and truncation *before* any expensive key
  derivation. It is not a security feature.
* Readers should be forgiving: ignore lines starting with `#`, blank lines,
  whitespace and `-` inside the data, and accept lower case.
* Length: 10 + 167 = 177 characters.

A `.seedlock` file is that text on one line, preceded by two `#` comment lines
explaining what the file is. Files are UTF-8 (ASCII in practice).

A backup's QR code holds the text form, and may carry the backup's name on a
second line, so that the device that scans it can suggest that name:

```
SEEDLOCK1:KNGEWAIB...
# name: My wallet
```

The name is one line of UTF-8 text, at most 100 characters, after `# name: `.
It is not encrypted: whoever sees the code can read it, as whoever sees a file
can read its file name. Because readers ignore lines that start with `#`, such a
code reads as the same backup in every reader, also in those that came before the
name line. Since version 0.2.0 the phone app and the command-line program (menu 8,
`seedlock qr`) write it. When they read such a code, the phone app suggests the
name, and the command-line program uses it for the files it saves (unless the same
backup is in its backups folder, whose name it then uses).

## 7. Known-answer test vector

These inputs use deliberately cheap Argon2 settings; they are for testing only.

```
password : correct-horse-battery-staple
entropy  : 000102030405060708090a0b0c0d0e0f
           (mnemonic: abandon amount liar amount expire adjust cage candy arch gather drum buyer)
salt     : 101112131415161718191a1b1c1d1e1f
nonce    : 202122232425262728292a2b2c2d2e2f3031323334353637
argon2id : m = 256 KiB, t = 1, p = 1

text     : SEEDLOCK1:KNGEWAIBAAAQAAABAEIBCEQTCQKRMFYYDENBWHA5DYPSAIJCEMSCKJRHFAUSUKZMFUXC6MBRGIZTINJWG74EM5MYIBOQAXY5PNR2F3S4BVA3F2QPGIKPZGVH2UVQ57VYCGYH6BB7JYQLXMSL5Q2OQKODKWGCWERO6WKGI5Y
```

This vector is checked by the Rust test-suite and by `python3 tools/recover.py --selftest`,
which uses different cryptographic libraries (argon2-cffi + libsodium). If either
ever fails, the format has changed and existing backups would stop opening.

## 8. Recovering without SeedLock

```
pip install argon2-cffi pynacl mnemonic
python3 tools/recover.py my-wallet.seedlock
```

Any language works: the only primitives needed are SHA-256, Base32, Argon2id
and XChaCha20-Poly1305 (libsodium's `crypto_aead_xchacha20poly1305_ietf`).

## 9. Versioning

Byte 3 is the format version. A reader that sees a version it does not know must
stop and say so; it must never guess. New versions will get a new text prefix
(`SEEDLOCK2:`), so old and new backups can never be confused.
