#!/usr/bin/env python3
"""Writes the silent payment test vectors that crates/seedlock-core/tests/silent_payments.rs reads, from the BIPs' own.

    git clone https://github.com/bitcoin/bips
    python3 tools/silent_vectors.py bips crates/seedlock-core/tests/data

It reads bip-0352/send_and_receive_test_vectors.json, bip-0374/test_vectors_*.csv and
bip-0375/bip375_test_vectors.json, and writes them as plain text lines (no JSON library needed in Rust), keeping only
what the tests use. The CSV files are copied as they are. The BIPs and their test vectors are under the BSD 2-clause
license (see each BIP).
"""
import json
import os
import shutil
import subprocess
import sys


def commit(bips):
    try:
        command = ["git", "-C", bips, "rev-parse", "--short", "HEAD"]
        done = subprocess.run(command, capture_output=True, text=True, check=True)
        return done.stdout.strip()
    except Exception:
        return "unknown"


def field(value):
    return value if value else "-"


def bip352(bips, out):
    vectors = json.load(open(os.path.join(bips, "bip-0352", "send_and_receive_test_vectors.json"), encoding="utf-8"))
    lines = [f"# BIP 352's test vectors (bitcoin/bips {commit(bips)}, bip-0352/send_and_receive_test_vectors.json),",
             "# written by tools/silent_vectors.py. send: in = txid vout prevout-script scriptSig witness private-key;",
             "# to = a recipient (and how many outputs it gets, when more than one); out = one accepted set of output",
             "# keys (x-only). receive: keys = scan and spend private keys; label = a label in use; address = an",
             "# expected address; found = an output key and its tweak."]
    for v in vectors:
        for s in v["sending"]:
            g, e = s["given"], s["expected"]
            lines.append(f"send {v['comment']}")
            for i in g["vin"]:
                lines.append(" ".join(["in", i["txid"], str(i["vout"]), i["prevout"]["scriptPubKey"]["hex"],
                                       field(i["scriptSig"]), field(i["txinwitness"]), i["private_key"]]))
            for r in g["recipients"]:
                address = r["address"] if isinstance(r, dict) else r
                count = r.get("count", 1) if isinstance(r, dict) else 1
                lines.append(f"to {address}" + (f" {count}" if count != 1 else ""))
            for outputs in e["outputs"]:
                lines.append(" ".join(["out"] + outputs))
            lines.append("end")
        for r in v["receiving"]:
            g, e = r["given"], r["expected"]
            lines.append(f"receive {v['comment']}")
            lines.append(f"keys {g['key_material']['scan_priv_key']} {g['key_material']['spend_priv_key']}")
            for m in g["labels"]:
                lines.append(f"label {m}")
            for a in e["addresses"]:
                lines.append(f"address {a}")
            for o in e.get("outputs", []):
                lines.append(f"found {o['pub_key']} {o['priv_key_tweak']}")
            lines.append("end")
    open(os.path.join(out, "bip352.txt"), "w", encoding="utf-8").write("\n".join(lines) + "\n")


def bip375(bips, out):
    vectors = json.load(open(os.path.join(bips, "bip-0375", "bip375_test_vectors.json"), encoding="utf-8"))
    lines = [f"# BIP 375's test vectors (bitcoin/bips {commit(bips)}, bip-0375/bip375_test_vectors.json), written by",
             "# tools/silent_vectors.py: whether the PSBT is valid, its description, the PSBT, and for each input",
             "# whose key the test generator used: key = input-index private-key public-key."]
    for kind in ("valid", "invalid"):
        for v in vectors[kind]:
            lines.append(f"{kind} {v['description']}")
            lines.append(f"psbt {v['psbt']}")
            for i in v.get("supplementary", {}).get("inputs", []):
                if i.get("private_key"):
                    lines.append(f"key {i['input_index']} {i['private_key']} {i.get('public_key') or '-'}")
            lines.append("end")
    open(os.path.join(out, "bip375.txt"), "w", encoding="utf-8").write("\n".join(lines) + "\n")


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    bips, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    bip352(bips, out)
    for name in ("generate_proof", "verify_proof"):
        source = os.path.join(bips, "bip-0374", f"test_vectors_{name}.csv")
        shutil.copyfile(source, os.path.join(out, f"bip374_{name}.csv"))
    bip375(bips, out)
    print("written into", out)


if __name__ == "__main__":
    main()
