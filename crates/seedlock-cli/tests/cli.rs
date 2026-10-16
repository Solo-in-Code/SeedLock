//! End-to-end tests: run the real `seedlock` binary with piped input.

use seedlock_core::bitcoin::bip32::DerivationPath;
use seedlock_core::bitcoin::hashes::Hash;
use seedlock_core::bitcoin::psbt::Psbt;
use seedlock_core::bitcoin::{
    absolute, transaction, Address, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness,
};
use seedlock_core::{backup, mnemonic, multisig::MultisigWallet, psbt as core_psbt, wallet::{ScriptType, Wallet}, Network};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::str::FromStr;

const WORDS: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const OTHER_WORDS: &str = "legal winner thank year wave sausage worth useful legal winner thank yellow";
const PW: &str = "Correct-Horse-Battery-Staple";
const FP: &str = "73c5da0a";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("seedlock-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str], stdin: &str) -> Output {
    run_in(None, args, stdin)
}

/// Run in a given working directory (the "seedlocker backups" folder is created there).
fn run_in(cwd: Option<&Path>, args: &[&str], stdin: &str) -> Output {
    run_env(cwd, &[], args, stdin)
}

fn run_env(cwd: Option<&Path>, envs: &[(&str, &str)], args: &[&str], stdin: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_seedlock"));
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.envs(envs.iter().copied());
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cannot start seedlock");
    // A child that fails fast (bad arguments, for example - this is exactly what some tests check
    // for) can exit and close its end of the pipe before this finishes writing to it. That is a
    // real, working program, not a broken test, so a broken pipe here is not a panic.
    if let Err(e) = child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            panic!("could not write to seedlock's stdin: {e}");
        }
    }
    child.wait_with_output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn make_backup(dir: &Path, words: &str, name: &str) -> PathBuf {
    let file = dir.join(name);
    let o = run(&["encrypt", "--out", s(&file), "--kdf", "light"], &format!("{words}\n{PW}\n"));
    assert!(o.status.success(), "encrypt failed: {}", err(&o));
    file
}

#[test]
fn encrypt_verify_decrypt_roundtrip() {
    let dir = tmp("roundtrip");
    let file = make_backup(&dir, WORDS, "w.seedlock");

    // the file is a readable text file containing the backup
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.lines().any(|l| l.starts_with("SEEDLOCK1:")));
    assert!(!text.contains("abandon"), "the mnemonic must never appear in the file");

    let v = run(&["verify", s(&file), "--fingerprint", FP], &format!("{PW}\n"));
    assert!(v.status.success(), "{}", err(&v));
    assert_eq!(out(&v).trim(), FP);

    let d = run(&["decrypt", s(&file), "--raw"], &format!("{PW}\n"));
    assert!(d.status.success());
    assert_eq!(out(&d).trim(), WORDS);

    // the backup text itself (e.g. scanned from a QR code) works instead of a path
    let line = text.lines().find(|l| l.starts_with("SEEDLOCK1:")).unwrap();
    let d2 = run(&["decrypt", line, "--raw"], &format!("{PW}\n"));
    assert_eq!(out(&d2).trim(), WORDS);
}

#[test]
fn wrong_password_and_wrong_fingerprint_are_reported() {
    let dir = tmp("wrong");
    let file = make_backup(&dir, WORDS, "w.seedlock");

    let bad = run(&["decrypt", s(&file), "--raw"], "not-the-password\n");
    assert_eq!(bad.status.code(), Some(1));
    assert!(err(&bad).contains("wrong password"), "{}", err(&bad));
    assert!(out(&bad).is_empty(), "nothing may be printed on stdout when decryption fails");

    let mismatch = run(&["verify", s(&file), "--fingerprint", "deadbeef"], &format!("{PW}\n"));
    assert_eq!(mismatch.status.code(), Some(2));
    assert!(err(&mismatch).contains("MISMATCH"));
}

#[test]
fn compare_with_paper_backup() {
    let dir = tmp("compare");
    let file = make_backup(&dir, WORDS, "w.seedlock");
    let same = run(&["verify", s(&file), "--compare-words"], &format!("{PW}\n{WORDS}\n"));
    assert!(same.status.success(), "{}", err(&same));
    assert!(err(&same).contains("MATCH"));
    let different = run(&["verify", s(&file), "--compare-words"], &format!("{PW}\n{OTHER_WORDS}\n"));
    assert_eq!(different.status.code(), Some(2));
}

#[test]
fn never_overwrites_an_existing_backup() {
    let dir = tmp("overwrite");
    let file = make_backup(&dir, WORDS, "w.seedlock");
    let before = std::fs::read(&file).unwrap();
    let again = run(&["encrypt", "--out", s(&file), "--kdf", "light"], &format!("{OTHER_WORDS}\n{PW}\n"));
    assert!(!again.status.success());
    assert!(err(&again).contains("already exists"));
    assert!(!err(&again).contains("--force"), "the message must not suggest overwriting");
    assert_eq!(std::fs::read(&file).unwrap(), before, "the existing backup must be untouched");
    // there is no --force option at all
    let forced = run(&["encrypt", "--out", s(&file), "--kdf", "light", "--force"], &format!("{WORDS}\n{PW}\n"));
    assert!(!forced.status.success());
    assert!(err(&forced).contains("--force"), "clap reports the unknown option");
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn backups_go_into_the_seedlocker_backups_folder() {
    let dir = tmp("folder");
    let o = run_in(Some(&dir), &["encrypt", "--kdf", "light"], &format!("{WORDS}\n{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let folder = dir.join("seedlocker backups");
    let file = folder.join("seedlock-backup.seedlock");
    assert!(file.is_file(), "the backup must be inside the folder");
    assert!(err(&o).contains("seedlocker backups"), "the full path is shown: {}", err(&o));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing else is created next to it");

    // a backup can be opened by its name, with or without the extension
    for name in ["seedlock-backup", "seedlock-backup.seedlock"] {
        let d = run_in(Some(&dir), &["decrypt", name, "--raw"], &format!("{PW}\n"));
        assert_eq!(out(&d).trim(), WORDS, "{name}: {}", err(&d));
    }

    // the same name again: refused, nothing overwritten, and the message does not suggest --force
    let before = std::fs::read(&file).unwrap();
    let again = run_in(Some(&dir), &["encrypt", "--kdf", "light"], &format!("{OTHER_WORDS}\n{PW}\n"));
    assert!(!again.status.success());
    assert!(err(&again).contains("already exists") && !err(&again).contains("--force"), "{}", err(&again));
    assert_eq!(std::fs::read(&file).unwrap(), before);

    // an unknown name lists the backups that do exist
    let unknown = run_in(Some(&dir), &["decrypt", "nope", "--raw"], "");
    assert!(!unknown.status.success());
    assert!(err(&unknown).contains("seedlock-backup"), "{}", err(&unknown));
}

#[test]
fn dice_needs_50_or_100_rolls() {
    let dir = tmp("dice-counts");
    let attempt = |words: &str, rolls: usize, extra: &[&str], name: &str| {
        let file = dir.join(name);
        let mut args = vec!["new", "--words", words, "--kdf", "light", "--dice", "--out", s(&file)];
        args.extend_from_slice(extra);
        let o = run(&args, &format!("{}\n{PW}\n", "3".repeat(rolls)));
        (o.status.success(), err(&o), file.exists())
    };
    assert!(attempt("12", 50, &[], "a").0, "12 words need 50 rolls");
    assert!(attempt("24", 100, &[], "b").0, "24 words need 100 rolls");

    let (ok, msg, created) = attempt("24", 99, &[], "c");
    assert!(!ok && !created, "99 rolls are not enough for 24 words");
    assert!(msg.contains("99 of the 100 rolls"), "the message reports the counter: {msg}");
    let (ok, msg, _) = attempt("12", 49, &[], "d");
    assert!(!ok && msg.contains("49 of the 50 rolls"), "{msg}");

    // SeedSigner's own number (99) is kept for its dice-only mode
    assert!(attempt("24", 99, &["--dice-only"], "e").0);
}

#[test]
fn bad_input_gets_helpful_errors() {
    let dir = tmp("errors");
    let f = dir.join("x.seedlock");
    let typo = WORDS.replacen("abandon", "abandn", 1);
    let o = run(&["encrypt", "--out", s(&f), "--kdf", "light"], &format!("{typo}\n{PW}\n"));
    assert!(!o.status.success());
    assert!(err(&o).contains("did you mean: abandon"), "{}", err(&o));
    assert!(!f.exists());

    let short = run(&["encrypt", "--out", s(&f), "--kdf", "light"], &format!("{WORDS}\nshort\n"));
    assert!(!short.status.success());
    assert!(err(&short).contains("at least"));

    let not_backup = dir.join("random.txt");
    std::fs::write(&not_backup, "hello world").unwrap();
    let o = run(&["inspect", s(&not_backup)], "");
    assert!(!o.status.success());
    assert!(err(&o).contains("not a SeedLock backup"));
}

#[test]
fn info_prints_known_addresses_and_descriptors() {
    let dir = tmp("info");
    let file = make_backup(&dir, WORDS, "w.seedlock");
    let o = run(&["info", s(&file), "--count", "2"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(text.contains(FP));
    assert!(text.contains("[73c5da0a/84h/0h/0h]xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYVUhLv1VMrjPC7PW6V"));
    assert!(text.contains("bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"));
    assert!(text.contains("bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g"));
    assert!(text.contains("wpkh([73c5da0a/84h/0h/0h]xpub"));

    let tr = run(&["info", s(&file), "--script", "tr", "--count", "1"], &format!("{PW}\n"));
    assert!(out(&tr).contains("bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr"));
}

#[test]
fn new_seed_creates_a_working_backup() {
    let dir = tmp("new");
    let file = dir.join("new.seedlock");
    let o = run(&["new", "--words", "12", "--kdf", "light", "--out", s(&file)], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let d = run(&["decrypt", s(&file), "--raw"], &format!("{PW}\n"));
    let phrase = out(&d);
    assert_eq!(phrase.split_whitespace().count(), 12);
    mnemonic::parse(&phrase).expect("generated mnemonic must be valid");
    let bad = run(&["new", "--words", "13"], "");
    assert!(!bad.status.success());
}

#[test]
fn dice_only_is_deterministic_and_mixed_dice_is_not() {
    let dir = tmp("dice");
    let rolls = "3".repeat(50);
    let words = |name: &str, extra: &[&str]| {
        let file = dir.join(name);
        let mut args = vec!["new", "--words", "12", "--kdf", "light", "--dice", "--out", s(&file)];
        args.extend_from_slice(extra);
        let o = run(&args, &format!("{rolls}\n{PW}\n"));
        assert!(o.status.success(), "{}", err(&o));
        out(&run(&["decrypt", s(&file), "--raw"], &format!("{PW}\n")))
    };
    assert_eq!(words("a", &["--dice-only"]), words("b", &["--dice-only"]));
    assert_ne!(words("c", &[]), words("d", &[]));
}

#[test]
fn qr_and_svg_output() {
    let dir = tmp("qr");
    let file = make_backup(&dir, WORDS, "w.seedlock");
    let svg = dir.join("w.svg");
    let o = run_in(Some(&dir), &["qr", s(&file), "--svg", s(&svg)], "");
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains('█'));
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
}

#[test]
fn qr_is_also_saved_as_a_picture_in_the_backups_folder() {
    let dir = tmp("qrpng");
    let file = make_backup(&dir, WORDS, "my wallet.seedlock");
    let picture = dir.join("seedlocker backups").join("my wallet-qr.png");
    let o = run_in(Some(&dir), &["qr", s(&file)], "");
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("saved as a picture"), "{}", err(&o));
    let bytes = std::fs::read(&picture).expect("the picture is in the backups folder");
    assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // asking again keeps the same picture: nothing is overwritten, nothing piles up
    let again = run_in(Some(&dir), &["qr", s(&file)], "");
    assert!(again.status.success());
    assert_eq!(std::fs::read(&picture).unwrap(), bytes);
    let pictures = std::fs::read_dir(dir.join("seedlocker backups")).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "png")).count();
    assert_eq!(pictures, 1);

    // if a different picture already has that name, it is left alone and a new name is used
    std::fs::write(&picture, b"not the same").unwrap();
    let third = run_in(Some(&dir), &["qr", s(&file)], "");
    assert!(third.status.success());
    assert_eq!(std::fs::read(&picture).unwrap(), b"not the same");
    assert!(dir.join("seedlocker backups").join("my wallet-qr-2.png").is_file());
}

/// The text of the QR code in a PNG picture that SeedLock saved (its own format: 1 bit per pixel, uncompressed),
/// read back with rqrr.
fn png_qr_text(path: &Path) -> String {
    let png = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let (mut pos, mut side, mut idat) = (8, 0usize, Vec::new());
    while pos < png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let data = &png[pos + 8..pos + 8 + len];
        match &png[pos + 4..pos + 8] {
            b"IHDR" => side = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize,
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        pos += 12 + len;
    }
    // zlib with stored blocks: a byte that says whether it is the last block, its length, the length inverted, the bytes
    let (mut raw, mut p) = (Vec::new(), 2);
    loop {
        let last = idat[p] == 1;
        let len = u16::from_le_bytes([idat[p + 1], idat[p + 2]]) as usize;
        raw.extend_from_slice(&idat[p + 5..p + 5 + len]);
        p += 5 + len;
        if last {
            break;
        }
    }
    let stride = side.div_ceil(8);
    let dark = |x: usize, y: usize| raw[y * (stride + 1) + 1 + x / 8] & (0x80 >> (x % 8)) == 0;
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| if dark(x, y) { 0 } else { 255 });
    let grids = image.detect_grids();
    assert_eq!(grids.len(), 1, "one QR code in {}", path.display());
    grids[0].decode().unwrap().1
}

#[test]
fn the_backup_qr_code_carries_the_backups_name() {
    let dir = tmp("qrname");
    let file = make_backup(&dir, WORDS, "my wallet.seedlock");
    let contents = std::fs::read_to_string(&file).unwrap();
    let o = run_in(Some(&dir), &["qr", s(&file)], "");
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("holds the ENCRYPTED backup and its name, \"my wallet\""), "{}", err(&o));
    let read = png_qr_text(&dir.join("seedlocker backups").join("my wallet-qr.png"));
    assert_eq!(read, backup::qr_text(&contents, "my wallet").unwrap());
    assert_eq!(read.lines().nth(1), Some("# name: my wallet"), "the name on a line of its own after the backup line");
    assert_eq!(backup::from_text(&read).unwrap(), backup::from_text(&contents).unwrap(), "and it reads as the same backup");
    // a pasted backup line carries no name: its code is the line alone, as before
    let line = contents.lines().find(|l| l.starts_with("SEEDLOCK1:")).unwrap();
    let o = run_in(Some(&dir), &["qr", line], "");
    assert!(o.status.success() && err(&o).contains("ENCRYPTED backup only"), "{}", err(&o));
    assert_eq!(png_qr_text(&dir.join("seedlocker backups").join("seedlock-backup-qr.png")), line);
}

/// A stand-in routing table of an offline computer, so that the menu does not depend on this one's network.
fn offline_net(name: &str) -> PathBuf {
    let net = tmp(name);
    std::fs::write(net.join("route"), "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n").unwrap();
    std::fs::write(net.join("ipv6_route"), "").unwrap();
    net
}

#[test]
fn menu_5_shows_the_wallet_for_sparrow_as_a_qr_code_that_names_the_keystore() {
    let dir = tmp("menu5");
    let backups = dir.join("seedlocker backups");
    std::fs::create_dir_all(&backups).unwrap();
    make_backup(&backups, WORDS, "my wallet.seedlock");
    let net = offline_net("menu5-net");
    // menu 5, the only backup (Enter), no passphrase, its password, mainnet (no transaction tells), then quit
    let o = run_env(Some(&dir), &[("SEEDLOCK_TEST_NET_DIR", s(&net))], &[], &format!("5\n\nn\n{PW}\nm\nq\n"));
    assert!(o.status.success(), "{}", err(&o));
    let shown = out(&o);
    assert!(shown.contains("names its keystore \"my wallet\"") && shown.contains("type \"my wallet\" there too"), "{shown}");
    assert!(shown.contains("wpkh([73c5da0a/84h/0h/0h]xpub6CatWdiZ"), "the descriptor to paste is still there: {shown}");
    let expected = wallet(WORDS).crypto_output_ur(ScriptType::Wpkh, 0, "my wallet").unwrap();
    assert_eq!(png_qr_text(&backups.join("my wallet-for-sparrow-qr.png")), expected, "the picture is Sparrow's UR crypto-output");
    let saved = std::fs::read_to_string(backups.join("my wallet-for-sparrow.txt")).unwrap();
    assert!(saved.contains(&format!("As a QR code (UR crypto-output):\n  {expected}")), "{saved}");
}

#[test]
fn password_and_final_word_helpers() {
    let p = run(&["password", "--words", "6"], "");
    assert!(p.status.success());
    assert_eq!(out(&p).trim().split('-').count(), 6);
    assert!(!run(&["password", "--words", "2"], "").status.success());

    let f = run(&["final-word"], &format!("{}\n", WORDS.rsplit_once(' ').unwrap().0));
    assert!(f.status.success());
    assert!(out(&f).starts_with("128 valid final words"));
}

// ---------------------------------------------------------------------------
// PSBT signing
// ---------------------------------------------------------------------------

fn wallet(words: &str) -> Wallet {
    Wallet::new(&mnemonic::parse(words).unwrap(), "", Network::Bitcoin).unwrap()
}

/// Spend 100_000 sat from m/84'/0'/0'/0/0: 60_000 to an outside address,
/// 39_000 back to m/84'/0'/0'/1/0, fee 1_000.
fn build_psbt(w: &Wallet) -> Psbt {
    build_psbt_for(w, 0, "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4")
}

/// The same spend with the key paths of coin type `coin` (0 = mainnet, 1 = a test network).
fn build_psbt_for(w: &Wallet, coin: u32, outside: &str) -> Psbt {
    let in_path = DerivationPath::from_str(&format!("m/84'/{coin}'/0'/0/0")).unwrap();
    let ch_path = DerivationPath::from_str(&format!("m/84'/{coin}'/0'/1/0")).unwrap();
    let in_pk = w.public_key_at(&in_path).unwrap();
    let ch_pk = w.public_key_at(&ch_path).unwrap();
    let outside = Address::from_str(outside).unwrap().assume_checked();
    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint { txid: Txid::from_byte_array([0xaa; 32]), vout: 1 },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut { value: Amount::from_sat(60_000), script_pubkey: outside.script_pubkey() },
            TxOut { value: Amount::from_sat(39_000), script_pubkey: w.script_pubkey(ScriptType::Wpkh, &ch_pk) },
        ],
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
    psbt.inputs[0].witness_utxo =
        Some(TxOut { value: Amount::from_sat(100_000), script_pubkey: w.script_pubkey(ScriptType::Wpkh, &in_pk) });
    psbt.inputs[0].bip32_derivation.insert(in_pk, (w.fingerprint(), in_path));
    psbt.outputs[1].bip32_derivation.insert(ch_pk, (w.fingerprint(), ch_path));
    psbt
}

#[test]
fn sign_psbt_end_to_end() {
    let dir = tmp("sign");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let psbt_file = dir.join("send.psbt");
    std::fs::write(&psbt_file, core_psbt::encode_base64(&build_psbt(&wallet(WORDS)))).unwrap(); // base64 text file

    let o = run(&["sign", s(&backup), "--psbt", s(&psbt_file), "--yes"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let review = err(&o);
    assert!(review.contains("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"));
    assert!(review.contains("SENT TO SOMEONE ELSE"));
    assert!(review.contains("change back to you"));
    assert!(review.contains("Fee      : 0.00001000 BTC"));

    let signed_file = dir.join("send.signed.psbt");
    let signed = core_psbt::decode(&std::fs::read(&signed_file).unwrap()).unwrap();
    assert!(!signed.inputs[0].partial_sigs.is_empty(), "the file must contain the signature");
    // stdout carries the same PSBT as base64
    assert_eq!(core_psbt::decode(out(&o).trim().as_bytes()).unwrap(), signed);
}

#[test]
fn the_network_is_found_from_the_transaction() {
    let dir = tmp("network");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    // a test-network transaction, signed without --network: recognised, shown with testnet addresses, no warning
    let test_psbt = dir.join("test.psbt");
    let test_tx = build_psbt_for(&wallet(WORDS), 1, "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx");
    std::fs::write(&test_psbt, core_psbt::encode_base64(&test_tx)).unwrap();
    let o = run(&["sign", s(&backup), "--psbt", s(&test_psbt), "--yes"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let review = err(&o);
    assert!(review.contains("Network: a test network (testnet or signet), found from the transaction"), "{review}");
    assert!(review.contains("tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx") && review.contains("change back to you"), "{review}");
    assert!(!review.contains("does not match the selected network"), "{review}");
    assert!(!core_psbt::decode(&std::fs::read(dir.join("test.signed.psbt")).unwrap()).unwrap().inputs[0].partial_sigs.is_empty());
    // a mainnet transaction: said plainly
    let main_psbt = dir.join("main.psbt");
    std::fs::write(&main_psbt, core_psbt::encode_base64(&build_psbt(&wallet(WORDS)))).unwrap();
    let o = run(&["sign", s(&backup), "--psbt", s(&main_psbt), "--yes"], &format!("{PW}\n"));
    assert!(o.status.success() && err(&o).contains("Network: mainnet (real bitcoin), found from the transaction"), "{}", err(&o));
    // --network still wins, and a transaction for another network is still flagged
    let o = run(&["sign", s(&backup), "--psbt", s(&test_psbt), "--network", "mainnet", "--yes"], &format!("{PW}\n"));
    assert!(err(&o).contains("Network: mainnet") && err(&o).contains("does not match the selected network"), "{}", err(&o));
    // no key paths at all, and no terminal to ask: a clear error instead of a guess
    let bare = dir.join("bare.psbt");
    let mut bare_tx = build_psbt(&wallet(WORDS));
    bare_tx.inputs[0].bip32_derivation.clear();
    bare_tx.outputs[1].bip32_derivation.clear();
    std::fs::write(&bare, core_psbt::encode_base64(&bare_tx)).unwrap();
    let o = run(&["sign", s(&backup), "--psbt", s(&bare), "--yes"], &format!("{PW}\n"));
    assert!(!o.status.success() && err(&o).contains("no key paths") && err(&o).contains("--network"), "{}", err(&o));
}

const THIRD_WORDS: &str = "letter advice cage absurd amount doctor acoustic avoid letter advice cage above";

/// Three seeds as a 2-of-3 native SegWit multisig on a test network, as Sparrow describes it.
fn vault_descriptor() -> String {
    let account = DerivationPath::from_str("m/48'/1'/0'/2'").unwrap();
    let keys: Vec<String> = [WORDS, OTHER_WORDS, THIRD_WORDS]
        .iter()
        .map(|words| {
            let w = Wallet::new(&mnemonic::parse(words).unwrap(), "", Network::Testnet).unwrap();
            format!("[{}/48h/1h/0h/2h]{}/<0;1>/*", w.fingerprint(), w.xpub_at(&account).unwrap())
        })
        .collect();
    let body = format!("wsh(sortedmulti(2,{}))", keys.join(","));
    format!("{body}#{}", seedlock_core::wallet::descriptor_checksum(&body).unwrap())
}

/// Spend 100_000 sat from the vault's receive address 0: 60_000 out, 39_000 to its change address 0.
fn vault_psbt(ms: &MultisigWallet) -> Psbt {
    let key_paths = |chain: u32, index: u32| {
        [WORDS, OTHER_WORDS, THIRD_WORDS]
            .iter()
            .map(|words| {
                let w = Wallet::new(&mnemonic::parse(words).unwrap(), "", Network::Testnet).unwrap();
                let path = DerivationPath::from_str(&format!("m/48'/1'/0'/2'/{chain}/{index}")).unwrap();
                (w.public_key_at(&path).unwrap(), (w.fingerprint(), path))
            })
            .collect()
    };
    let outside = Address::from_str("tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx").unwrap().assume_checked();
    let tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint { txid: Txid::from_byte_array([0xcc; 32]), vout: 1 },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut { value: Amount::from_sat(60_000), script_pubkey: outside.script_pubkey() },
            TxOut { value: Amount::from_sat(39_000), script_pubkey: ms.script_pubkey(1, 0).unwrap() },
        ],
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
    psbt.inputs[0].witness_utxo = Some(TxOut { value: Amount::from_sat(100_000), script_pubkey: ms.script_pubkey(0, 0).unwrap() });
    psbt.inputs[0].witness_script = Some(ms.witness_script(0, 0).unwrap());
    psbt.inputs[0].bip32_derivation = key_paths(0, 0);
    psbt.outputs[1].witness_script = Some(ms.witness_script(1, 0).unwrap());
    psbt.outputs[1].bip32_derivation = key_paths(1, 0);
    psbt
}

#[test]
fn signs_a_registered_2_of_3_multisig_one_cosigner_at_a_time() {
    let dir = tmp("multisig");
    let backups = dir.join("seedlocker backups");
    std::fs::create_dir_all(&backups).unwrap();
    let a = make_backup(&backups, WORDS, "a.seedlock");
    let b = make_backup(&backups, OTHER_WORDS, "b.seedlock");
    let descriptor = vault_descriptor();
    // registered by an earlier version, next to the backups: moved into "multisig wallets" and used
    std::fs::write(backups.join("vault.multisig"), format!("# a registered wallet\n{descriptor}\n")).unwrap();
    std::fs::write(backups.join("a-cosigner.txt"), "a cosigner key shared earlier\n").unwrap();
    let ms = MultisigWallet::from_descriptor_text(&descriptor).unwrap();
    std::fs::write(dir.join("vault.psbt"), core_psbt::encode_base64(&vault_psbt(&ms))).unwrap();

    // cosigner A: the review shows the multisig, and A adds exactly one signature
    let o = run_in(Some(&dir), &["sign", s(&a), "--psbt", "vault.psbt", "--yes"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let review = err(&o);
    assert!(review.contains("(Moved into their own folders in \"seedlocker backups\": "), "{review}");
    assert!(backups.join("multisig wallets/vault.multisig").is_file() && !backups.join("vault.multisig").exists());
    assert!(backups.join("cosigner keys/a-cosigner.txt").is_file() && !backups.join("a-cosigner.txt").exists());
    assert!(review.contains("Network: a test network (testnet or signet), found from the transaction"), "{review}");
    assert!(review.contains("yours, from the multisig wallet \"vault\" (m/48'/1'/0'/2'/0/0)"), "{review}");
    assert!(review.contains("change back to the multisig wallet \"vault\" (verified from all its cosigners' keys: m/48'/1'/0'/2'/1/0)"), "{review}");
    assert!(review.contains("Multisig \"vault\", 2 of 3: this seed is cosigner") && review.contains("1 more cosigner must sign it too"), "{review}");
    assert!(review.contains("Leaving your wallet: 0.00060000 BTC"), "{review}");
    let signed_a = core_psbt::decode(&std::fs::read(dir.join("vault.signed.psbt")).unwrap()).unwrap();
    assert_eq!(signed_a.inputs[0].partial_sigs.len(), 1);

    // cosigner B signs A's file: now two signatures, which is enough
    let o = run_in(Some(&dir), &["sign", s(&b), "--psbt", "vault.signed.psbt", "--yes"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("That is enough: after this, Sparrow can finalize and broadcast the transaction."), "{}", err(&o));
    let signed_ab = core_psbt::decode(&std::fs::read(dir.join("vault.signed.signed.psbt")).unwrap()).unwrap();
    assert_eq!(signed_ab.inputs[0].partial_sigs.len(), 2);

    // without the registration, the same transaction is refused, with what to do
    let other = tmp("multisig-unregistered");
    std::fs::write(other.join("vault.psbt"), core_psbt::encode_base64(&vault_psbt(&ms))).unwrap();
    let o = run_in(Some(&other), &["sign", s(&a), "--psbt", "vault.psbt", "--yes"], &format!("{PW}\n"));
    assert!(!o.status.success() && err(&o).contains("not registered") && err(&o).contains("menu 10"), "{}", err(&o));
}

#[test]
fn shows_its_version_above_the_menu_and_with_dash_dash_version() {
    let version = env!("CARGO_PKG_VERSION");
    // a stand-in routing table of an offline computer, so the result does not depend on this one
    let net = tmp("version-net");
    std::fs::write(net.join("route"), "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n").unwrap();
    std::fs::write(net.join("ipv6_route"), "").unwrap();
    let o = run_env(None, &[("SEEDLOCK_TEST_NET_DIR", net.to_str().unwrap())], &[], "q\n");
    assert!(o.status.success(), "{}", err(&o));
    let shown = err(&o);
    let title = format!("=== SeedLock {version}: offline Bitcoin seed tool ===");
    let (at_title, at_menu) = (shown.find(&title), shown.find("What would you like to do?"));
    assert!(at_title.is_some() && at_menu.is_some() && at_title < at_menu, "the version must be in the title above the menu:\n{shown}");
    let o = run(&["--version"], "");
    assert!(o.status.success());
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), format!("seedlock {version}"));
}

/// Every version number that the README and the guides give for SeedLock ("SeedLock 0.1.1", "version 0.1.1",
/// "seedlock-0.1.1") is the program's own, so that a new version in Cargo.toml cannot leave an old number behind.
#[test]
fn the_readme_and_the_guides_show_this_version() {
    let version = env!("CARGO_PKG_VERSION");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = 0;
    for doc in ["README.md", "docs/GUIDE.md", "docs/DEVELOPMENT.md"] {
        let text = std::fs::read_to_string(root.join(doc)).unwrap();
        for word in ["SeedLock ", "version ", "seedlock-"] {
            for (at, _) in text.match_indices(word) {
                let number: String = text[at + word.len()..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                let number = number.trim_end_matches('.');
                if number.starts_with(|c: char| c.is_ascii_digit()) && number.contains('.') {
                    assert_eq!(number, version, "{doc} gives SeedLock's version as \"{word}{number}\", but the program is {version}");
                    found += 1;
                }
            }
        }
    }
    assert!(found >= 4, "the README and the guides show the version in at least 4 places, not {found}");
}

/// The release key's fingerprint is the same everywhere the README and the guides give it (in groups of four, or in one
/// piece in a command), and it is the fingerprint of the key in docs/release-key.asc, so that a new release key cannot
/// leave an old fingerprint or an old key file behind.
#[test]
fn the_release_key_fingerprint_is_the_same_everywhere() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let is_hex = |word: &str, len: usize| word.len() == len && word.chars().all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c));
    let mut fingerprints = Vec::new();
    for doc in ["README.md", "docs/GUIDE.md", "docs/DEVELOPMENT.md"] {
        let text = std::fs::read_to_string(root.join(doc)).unwrap();
        let words: Vec<&str> = text.split_whitespace().map(|w| w.trim_matches(|c: char| "`()[],.:;'\"".contains(c))).collect();
        let mut in_doc = 0;
        let mut i = 0;
        while i < words.len() {
            if is_hex(words[i], 40) {
                fingerprints.push((doc, words[i].to_string()));
                in_doc += 1;
            } else if i + 10 <= words.len() && words[i..i + 10].iter().all(|w| is_hex(w, 4)) {
                fingerprints.push((doc, words[i..i + 10].concat()));
                in_doc += 1;
                i += 9;
            }
            i += 1;
        }
        assert!(in_doc > 0, "{doc} does not give the release key's fingerprint");
    }
    let first = &fingerprints[0].1;
    for (doc, fingerprint) in &fingerprints {
        assert_eq!(fingerprint, first, "{doc} gives the release key's fingerprint as {fingerprint}, but README.md as {first}");
    }
    let of_file = openpgp_fingerprint(&std::fs::read_to_string(root.join("docs/release-key.asc")).unwrap());
    assert_eq!(&of_file, first, "docs/release-key.asc holds the key {of_file}, but the documents give {first}");
}

/// The version 4 fingerprint of the first key in an armored OpenPGP key file: SHA-1 over 0x99, the two-byte length and
/// the public key packet (RFC 4880, section 12.2).
fn openpgp_fingerprint(armored: &str) -> String {
    // the Base64 lines: after the empty line that ends the armor headers, up to the checksum ("=...") or the end line
    let base64: String = armored
        .lines()
        .skip_while(|l| !l.trim().is_empty())
        .skip(1)
        .take_while(|l| !l.starts_with('=') && !l.starts_with("-----"))
        .collect();
    let mut bytes = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in base64.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not Base64: {}", c as char),
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // the packet header: the old format (tag in bits 5-2) or the new format (tag in bits 5-0); the tag must be 6
    let (start, len) = if bytes[0] & 0xC0 == 0x80 {
        assert_eq!((bytes[0] >> 2) & 0x0F, 6, "the first packet must be a public key");
        match bytes[0] & 0x03 {
            0 => (2, usize::from(bytes[1])),
            1 => (3, usize::from(u16::from_be_bytes([bytes[1], bytes[2]]))),
            _ => (5, u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize),
        }
    } else {
        assert_eq!(bytes[0], 0xC6, "the first packet must be a public key");
        match bytes[1] {
            l @ 0..=191 => (2, usize::from(l)),
            l @ 192..=223 => (3, ((usize::from(l) - 192) << 8) + usize::from(bytes[2]) + 192),
            _ => (6, u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize),
        }
    };
    let packet = &bytes[start..start + len];
    assert_eq!(packet[0], 4, "a version 4 key");
    let mut hashed = vec![0x99, (len >> 8) as u8, len as u8];
    hashed.extend_from_slice(packet);
    seedlock_core::bitcoin::hashes::sha1::Hash::hash(&hashed).to_string().to_uppercase()
}

#[test]
fn declining_leaves_nothing_signed() {
    let dir = tmp("decline");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let psbt_file = dir.join("send.psbt");
    std::fs::write(&psbt_file, build_psbt(&wallet(WORDS)).serialize()).unwrap(); // binary file

    let o = run(&["sign", s(&backup), "--psbt", s(&psbt_file)], &format!("{PW}\nn\n"));
    assert!(o.status.success());
    assert!(err(&o).contains("Nothing was signed"));
    assert!(!dir.join("send.signed.psbt").exists());
}

#[test]
fn a_different_seed_cannot_sign() {
    let dir = tmp("otherseed");
    let other_backup = make_backup(&dir, OTHER_WORDS, "other.seedlock");
    let psbt_file = dir.join("send.psbt");
    std::fs::write(&psbt_file, core_psbt::encode_base64(&build_psbt(&wallet(WORDS)))).unwrap();

    let o = run(&["sign", s(&other_backup), "--psbt", s(&psbt_file), "--yes"], &format!("{PW}\n"));
    assert!(!o.status.success());
    assert!(err(&o).contains("none of the inputs belong"), "{}", err(&o));
    assert!(!dir.join("send.signed.psbt").exists());
}

#[test]
fn sign_can_show_the_signed_transaction_as_qr_frames() {
    let dir = tmp("signqr");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let psbt_file = dir.join("send.psbt");
    std::fs::write(&psbt_file, core_psbt::encode_base64(&build_psbt(&wallet(WORDS)))).unwrap();

    // Without a terminal the animated QR frames come out as text, one UR string per line.
    let o = run(&["sign", s(&backup), "--psbt", s(&psbt_file), "--yes", "--qr"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    let frames: Vec<String> = out(&o).lines().filter(|l| l.starts_with("ur:crypto-psbt/")).map(String::from).collect();
    assert!(frames.len() >= 4, "{frames:?}");
    let signed = core_psbt::decode(&std::fs::read(dir.join("send.signed.psbt")).unwrap()).unwrap();
    assert!(!signed.inputs[0].partial_sigs.is_empty());
    assert_eq!(core_psbt::from_ur_frames(&frames).unwrap(), signed, "the QR frames must carry the signed PSBT");

    // The static variant draws one QR code with the base64 PSBT.
    let o = run(&["sign", s(&backup), "--psbt", s(&psbt_file), "--yes", "--static-qr"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains('\u{2588}') || err(&o).contains('\u{2580}'), "a QR code must be drawn");

    // The two options exclude each other.
    let both = run(&["sign", s(&backup), "--psbt", s(&psbt_file), "--yes", "--qr", "--static-qr"], &format!("{PW}\n"));
    assert!(!both.status.success());
}

#[test]
fn signed_files_never_overwrite_each_other() {
    let dir = tmp("signnames");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let psbt_file = dir.join("send.psbt");
    std::fs::write(&psbt_file, core_psbt::encode_base64(&build_psbt(&wallet(WORDS)))).unwrap();
    for _ in 0..2 {
        let o = run(&["sign", s(&backup), "--psbt", s(&psbt_file), "--yes"], &format!("{PW}\n"));
        assert!(o.status.success(), "{}", err(&o));
    }
    assert!(dir.join("send.signed.psbt").is_file());
    assert!(dir.join("send.signed-2.psbt").is_file(), "the second signature gets a new name");

    let taken = run(&["sign", s(&backup), "--psbt", s(&psbt_file), "--yes", "--out", s(&dir.join("send.signed.psbt"))], &format!("{PW}\n"));
    assert!(!taken.status.success());
    assert!(err(&taken).contains("already exists"));
}

// ---------------------------------------------------------------------------
// Scanning the transaction with the camera (through a stand-in for ffmpeg)
// ---------------------------------------------------------------------------

/// A grey camera picture (1 byte per pixel) with one QR code in it.
fn qr_picture(text: &str, width: usize, height: usize) -> Vec<u8> {
    use qrcode::{types::Color, EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L).unwrap();
    let (n, colors, px, quiet) = (code.width(), code.to_colors(), 8usize, 4usize);
    let side = (n + 2 * quiet) * px;
    let (x0, y0) = ((width - side) / 2, (height - side) / 2);
    let mut grey = vec![200u8; width * height];
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = (x / px, y / px);
            let dark = mx >= quiet && mx < quiet + n && my >= quiet && my < quiet + n && colors[(my - quiet) * n + mx - quiet] == Color::Dark;
            grey[(y0 + y) * width + x0 + x] = if dark { 20 } else { 240 };
        }
    }
    grey
}

/// A script that behaves like `ffmpeg` reading a camera: it sends these QR pictures, one
/// after the other and in a loop, as raw grey pictures of 1280 x 720 through its output.
#[cfg(unix)]
fn fake_ffmpeg(dir: &Path, texts: &[String]) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let (w, h) = (1280usize, 720usize);
    let mut raw = Vec::new();
    for t in texts {
        raw.extend(qr_picture(t, w, h));
    }
    let pictures = dir.join("pictures.raw");
    std::fs::write(&pictures, &raw).unwrap();
    let script = dir.join("fake-ffmpeg");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = \"-version\" ] && {{ echo 'ffmpeg version test'; exit 0; }}; done\n\
             i=0\nwhile :; do\n  dd if='{}' bs={} skip=$((i % {})) count=1 2>/dev/null || exit 1\n  i=$((i+1)); sleep 0.05\ndone\n",
            pictures.display(),
            w * h,
            texts.len()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn sign_can_scan_the_transaction_with_the_camera() {
    let dir = tmp("scan");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let unsigned = build_psbt(&wallet(WORDS));
    let frames: Vec<String> = core_psbt::ur_frames(&unsigned).unwrap().iter().map(|f| f.to_uppercase()).collect();
    assert!(frames.len() >= 4, "an animated code, like Sparrow shows");
    let ffmpeg = fake_ffmpeg(&dir, &frames);

    let o = run_env(Some(&dir), &[("SEEDLOCK_FFMPEG", s(&ffmpeg))], &["sign", s(&backup), "--scan", "--yes"], &format!("{PW}\n"));
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("Received the whole transaction"), "{}", err(&o));
    assert!(err(&o).contains("TRANSACTION REVIEW"), "the review is shown before signing");

    // there was no input file, so the signed transaction is in the transactions folder
    let folder = dir.join("seedlocker transactions");
    let saved: Vec<PathBuf> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].file_name().unwrap().to_string_lossy().starts_with("signed-"));
    let signed = core_psbt::decode(&std::fs::read(&saved[0]).unwrap()).unwrap();
    assert_eq!(signed.unsigned_tx, unsigned.unsigned_tx, "the scanned transaction is what was signed");
    assert!(!signed.inputs[0].partial_sigs.is_empty());
    // the signed transaction also came out on stdout (there was no terminal)
    assert_eq!(core_psbt::decode(out(&o).trim().as_bytes()).unwrap(), signed);
}

#[cfg(unix)]
#[test]
fn a_scanned_backup_qr_code_gives_its_name_to_the_files_made_from_it() {
    let dir = tmp("scanname");
    let elsewhere = tmp("scanname-elsewhere");
    let file = make_backup(&elsewhere, WORDS, "w.seedlock");
    // the backup's QR code as SeedLock's phone app (and menu 8 from now on) shows it, of a backup that is not here
    let code = backup::qr_text(&std::fs::read_to_string(&file).unwrap(), "dave paper").unwrap();
    let ffmpeg = fake_ffmpeg(&dir, &[code]);
    let net = offline_net("scanname-net");
    let envs = [("SEEDLOCK_FFMPEG", s(&ffmpeg)), ("SEEDLOCK_NO_PREVIEW", "1"), ("SEEDLOCK_TEST_NET_DIR", s(&net))];
    // menu 10, 1 (this seed's key for a multisig wallet): c to scan the backup, no passphrase, its password,
    // a test network, the usual kind of wallet; then quit
    let o = run_env(Some(&dir), &envs, &[], &format!("10\n1\nc\nn\n{PW}\nt\n\nq\n"));
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("This QR code carries the backup's name, \"dave paper\""), "{}", err(&o));
    let keys = dir.join("seedlocker backups").join("cosigner keys");
    let text = std::fs::read_to_string(keys.join("dave paper-cosigner.txt")).unwrap();
    assert!(text.contains("names the keystore \"dave paper\""), "the keystore in Sparrow is named after it too: {text}");
    assert!(keys.join("dave paper-cosigner-qr.png").is_file());
}

#[test]
fn scanning_and_the_camera_check_explain_a_missing_ffmpeg() {
    let dir = tmp("noffmpeg");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let missing = [("SEEDLOCK_FFMPEG", "/nonexistent/ffmpeg-is-not-here")];
    let o = run_env(Some(&dir), &missing, &["sign", s(&backup), "--scan", "--yes"], &format!("{PW}\n"));
    assert!(!o.status.success());
    assert!(err(&o).contains("ffmpeg") && err(&o).contains("SEEDLOCK_FFMPEG") && err(&o).contains("winget") && err(&o).contains("pacman"), "{}", err(&o));
    let c = run_env(Some(&dir), &missing, &["camera"], "");
    assert!(!c.status.success());
    assert!(err(&c).contains("ffmpeg"), "{}", err(&c));
}

#[test]
fn sign_needs_a_file_or_the_camera_but_not_both() {
    let dir = tmp("source");
    let backup = make_backup(&dir, WORDS, "w.seedlock");
    let neither = run(&["sign", s(&backup), "--yes"], &format!("{PW}\n"));
    assert!(!neither.status.success());
    assert!(err(&neither).contains("--psbt"), "{}", err(&neither));
    let both = run(&["sign", s(&backup), "--psbt", "x.psbt", "--scan", "--yes"], &format!("{PW}\n"));
    assert!(!both.status.success());
}
