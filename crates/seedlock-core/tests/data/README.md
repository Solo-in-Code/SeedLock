# Silent payment test vectors

The test vectors of three BIPs, which `tests/silent_payments.rs` and the tests in `src/psbt_silent.rs` read. They
come from [bitcoin/bips](https://github.com/bitcoin/bips), commit `5aadddb`, and
[`tools/silent_vectors.py`](../../../../tools/silent_vectors.py) writes them here again from a copy of that
repository:

| File | From |
|---|---|
| `bip352.txt` | BIP 352 (silent payments): `bip-0352/send_and_receive_test_vectors.json`, as plain text lines |
| `bip374_generate_proof.csv`, `bip374_verify_proof.csv` | BIP 374 (DLEQ proofs): `bip-0374/test_vectors_*.csv`, as they are |
| `bip375.txt` | BIP 375 (silent payments in PSBTs): `bip-0375/bip375_test_vectors.json`, as plain text lines |

The BIPs and their test vectors are under the BSD 2-Clause license:

Copyright (c) the authors of BIP 352 (josibake, Ruben Somsen, Sebastian Falbesoner), BIP 374 (Andrew Toth, Ruben
Somsen, Sebastian Falbesoner) and BIP 375 (Andrew Toth, Ava Chow, josibake).

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
   disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following
   disclaimer in the documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
