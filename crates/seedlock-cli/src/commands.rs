//! Implementation of every command, plus the interactive menu.

use crate::cli::*;
use crate::{qrimg, qrwindow, scan};
use crate::ui::{self, fail, Res};
use qrcode::{EcLevel, QrCode};
use seedlock_core::bitcoin::psbt::Psbt;
use seedlock_core::{
    backup, entropy, mnemonic, psbt,
    psbt::OutputKind,
    multisig::{self, MultisigScript, MultisigWallet},
    wallet::{path_to_display, path_to_string, ScriptType, Wallet},
    Error as CoreError, Mnemonic, Network,
};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, IsTerminal, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Every backup lives in this folder, inside the directory SeedLock is started from.
const BACKUP_FOLDER: &str = "seedlocker backups";
const BACKUP_EXT: &str = "seedlock";
/// Inside the backups folder: each cosigner key shared for a multisig wallet (text and QR picture) ...
const COSIGNER_FOLDER: &str = "cosigner keys";
/// ... and each registered multisig wallet (`<name>.multisig`).
const MULTISIG_FOLDER: &str = "multisig wallets";
/// The hint shown in the "name your backup" prompt. It is not a real default.
const NAME_HINT: &str = "give-a-name-to-your-backup";
/// Used only when nobody can be asked (piped input and no --out).
const SCRIPT_DEFAULT_NAME: &str = "seedlock-backup";
const MAX_BACKUP_FILE: u64 = 64 * 1024;
const MAX_PSBT_FILE: u64 = 16 * 1024 * 1024;
/// Signed transactions that did not come from a file, and their QR pages, go in this folder.
const TX_FOLDER: &str = "seedlocker transactions";

// ===========================================================================
// The backups folder
// ===========================================================================

fn backup_folder() -> Res<PathBuf> {
    let here = std::env::current_dir().map_err(|e| format!("cannot find the current folder: {e}"))?;
    Ok(here.join(BACKUP_FOLDER))
}

/// The backups folder, created (private to this user) if it does not exist yet.
fn ensure_backup_folder() -> Res<PathBuf> {
    ensure_private_folder(backup_folder()?)
}

fn transactions_folder() -> Res<PathBuf> {
    let here = std::env::current_dir().map_err(|e| format!("cannot find the current folder: {e}"))?;
    Ok(here.join(TX_FOLDER))
}

fn ensure_tx_folder() -> Res<PathBuf> {
    ensure_private_folder(transactions_folder()?)
}

fn ensure_private_folder(dir: PathBuf) -> Res<PathBuf> {
    if dir.exists() && !dir.is_dir() {
        return fail(format!(
            "{} exists but is not a folder. Rename or move it, then try again.",
            dir.display()
        ));
    }
    if !dir.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&dir)
            .map_err(|e| format!("cannot create the folder {}: {e}", dir.display()))?;
    }
    Ok(dir)
}

/// (name, path) of every backup in a folder, sorted by name (same order every time).
fn list_backups_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case(BACKUP_EXT)))
        .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
        .collect();
    found.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then_with(|| a.0.cmp(&b.0)));
    found
}

fn list_backups() -> Vec<(String, PathBuf)> {
    backup_folder().map(|dir| list_backups_in(&dir)).unwrap_or_default()
}

/// The existing backup that a new backup called `name` would collide with, if any.
/// Upper and lower case count as the same: Windows, macOS, USB sticks (FAT/exFAT) and
/// most cloud folders treat "Wallet" and "wallet" as one file, so copying both would
/// silently replace one backup with the other.
fn taken_name(dir: &Path, name: &str) -> Option<String> {
    let wanted = name.to_lowercase();
    list_backups_in(dir)
        .into_iter()
        .map(|(n, _)| n)
        .find(|n| n.to_lowercase() == wanted)
        .or_else(|| backup_path(dir, name).exists().then(|| name.to_string()))
}

fn backup_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{BACKUP_EXT}"))
}

/// Turn what the person typed into a safe file name, or say what is wrong with it.
fn clean_backup_name(typed: &str) -> Result<String, String> {
    let mut name = typed.trim().trim_matches('"').trim_matches('\'').trim().to_string();
    // Somebody may type the extension too.
    if name.to_ascii_lowercase().ends_with(&format!(".{BACKUP_EXT}")) {
        name.truncate(name.len() - BACKUP_EXT.len() - 1);
        name = name.trim().to_string();
    }
    if name.is_empty() {
        return Err("Please type a name for your backup.".to_string());
    }
    if let Some(bad) = name.chars().find(|c| c.is_control() || "/\\:*?\"<>|".contains(*c)) {
        return Err(format!("A backup name cannot contain the character '{bad}'. Please choose another name."));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err("A backup name cannot start or end with a dot. Please choose another name.".to_string());
    }
    if name.chars().count() > 100 {
        return Err("That name is too long (100 characters at most). Please choose a shorter name.".to_string());
    }
    Ok(name)
}

fn exists_message(path: &Path) -> String {
    format!(
        "{} already exists. SeedLock never overwrites a file: please choose another name.",
        path.display()
    )
}

/// Where a new backup will be written. When a person is present, ask for a name
/// and keep asking until it is free: a taken name never ends the whole operation
/// (a new seed must not be lost because of it).
fn new_backup_path(out: &Option<PathBuf>) -> Res<PathBuf> {
    if let Some(path) = out {
        if path.exists() {
            return fail(exists_message(path));
        }
        return Ok(path.clone());
    }
    if !ui::interactive() {
        let path = backup_path(&ensure_backup_folder()?, SCRIPT_DEFAULT_NAME);
        if path.exists() {
            return fail(exists_message(&path));
        }
        return Ok(path);
    }
    let dir = backup_folder()?;
    eprintln!("\nYour backups are kept in this folder (it is created if it does not exist):");
    eprintln!("  {}", dir.display());
    loop {
        let typed = ui::read_line(&format!("\nSave the encrypted backup as [{NAME_HINT}]: "))?;
        match clean_backup_name(&typed) {
            Err(problem) => eprintln!("{problem}"),
            Ok(name) => match taken_name(&dir, &name) {
                Some(existing) => {
                    let note = if existing == name {
                        ""
                    } else {
                        " (names that differ only by upper or lower case count as the same)"
                    };
                    eprintln!("A backup called \"{existing}\" already exists{note}. Nothing was changed: please choose a different name.");
                }
                None => return Ok(backup_path(&ensure_backup_folder()?, &name)),
            },
        }
    }
}

/// Find a backup: a path, or the name of a backup inside the folder (with or without `.seedlock`).
fn find_backup_file(arg: &str) -> Res<PathBuf> {
    let dir = backup_folder()?;
    let candidates = [
        PathBuf::from(arg),
        dir.join(arg),
        backup_path(&dir, arg),
    ];
    if let Some(found) = candidates.iter().find(|c| c.is_file()) {
        return Ok(found.clone());
    }
    let wanted = arg.trim_end_matches(&format!(".{BACKUP_EXT}")).to_lowercase();
    let same_ignoring_case: Vec<PathBuf> = list_backups_in(&dir)
        .into_iter()
        .filter(|(n, _)| n.to_lowercase() == wanted)
        .map(|(_, p)| p)
        .collect();
    if let [only] = same_ignoring_case.as_slice() {
        return Ok(only.clone());
    }
    let names: Vec<String> = list_backups().into_iter().map(|(n, _)| n).collect();
    let hint = if names.is_empty() {
        format!(" There are no backups in {}.", dir.display())
    } else {
        format!(" The backups in {} are: {}.", dir.display(), names.join(", "))
    };
    fail(format!("cannot find a backup called \"{arg}\".{hint}"))
}

/// A backup argument is the name of a backup, a file path, or the `SEEDLOCK1:...` text itself.
fn load_backup_text(arg: &str) -> Res<String> {
    let arg = arg.trim();
    if arg.to_uppercase().starts_with(backup::TEXT_PREFIX) {
        return Ok(arg.to_string());
    }
    let path = find_backup_file(arg)?;
    let meta = fs::metadata(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if meta.len() > MAX_BACKUP_FILE {
        return fail(format!("{} is too large to be a SeedLock backup", path.display()));
    }
    Ok(fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?)
}

/// Create a brand-new file. An existing file is never touched.
fn write_new_file(path: &Path, contents: &[u8]) -> Res<()> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|e| {
        if e.kind() == ErrorKind::AlreadyExists {
            exists_message(path)
        } else {
            format!("cannot write {}: {e}", path.display())
        }
    })?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn full_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `dir/stem.ext`, or `dir/stem-2.ext`, `-3`, ... : the first name that is not taken yet.
fn unused_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem}-{n}.{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

// ===========================================================================
// Other shared helpers
// ===========================================================================

fn fingerprint_of(m: &Mnemonic) -> Res<String> {
    let wallet = Wallet::new(m, "", Network::Bitcoin)?;
    Ok(wallet.fingerprint().to_string())
}

fn network_name(n: Network) -> &'static str {
    match n {
        Network::Bitcoin => "mainnet",
        Network::Testnet => "testnet",
        Network::Signet => "signet",
        Network::Regtest => "regtest",
        _ => "other",
    }
}

/// Ask for the backup password and decrypt. Interactive users get three tries.
fn unlock(text: &str) -> Res<Mnemonic> {
    let info = backup::inspect_text(text)?;
    let attempts = if ui::interactive() { 3 } else { 1 };
    for attempt in 1..=attempts {
        let password = ui::secret("Backup password (hidden): ")?;
        eprintln!("Unlocking (Argon2id, {} MiB): this takes a moment...", info.kdf.memory_mib());
        match backup::decrypt_mnemonic(text, &password) {
            Ok(m) => return Ok(m),
            Err(CoreError::WrongPassword) if attempt < attempts => {
                eprintln!("Wrong password. Try again ({attempt}/{attempts}).");
            }
            Err(e) => return Err(e.into()),
        }
    }
    fail("could not unlock the backup")
}

fn ask_passphrase(ask: bool) -> Res<Zeroizing<String>> {
    if ask {
        ui::secret("BIP39 passphrase (hidden, case-sensitive): ")
    } else {
        Ok(Zeroizing::new(String::new()))
    }
}

fn read_mnemonic() -> Res<Mnemonic> {
    let attempts = if ui::interactive() { 5 } else { 1 };
    for attempt in 1..=attempts {
        let input = ui::secret("Enter your mnemonic words separated by spaces (hidden as you type):\n> ")?;
        match mnemonic::parse(&input) {
            Ok(m) => return Ok(m),
            Err(e) if attempt < attempts => eprintln!("  {e}\n  Please try again.\n"),
            Err(e) => return Err(e.into()),
        }
    }
    fail("no valid mnemonic entered")
}

fn show_qr(text: &str) -> Res<()> {
    if ui::can_animate() && qr_window_wanted() {
        let how = "Scan it with the camera of the other device (a phone, for example).";
        match show_in_window("SeedLock - backup QR code", &[text.to_string()], EcLevel::M, how) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(why) => eprintln!("\n(No QR window: {why}. The QR code is drawn in this terminal instead.)"),
        }
    }
    let art = ui::qr_terminal(text, EcLevel::M)?;
    // The full style (Apple's Terminal) is twice as big: make sure it fits before drawing it,
    // because a line too long for the window wraps around and breaks the code.
    if ui::qr_style() == ui::QrStyle::Full && ui::interactive() {
        apple_terminal_note();
        let (lines, columns) = ui::block_size(&art);
        if !ui::wait_for_room(lines + 2, columns + 1, "QR code")? {
            return Ok(());
        }
    }
    eprintln!("\n{art}");
    Ok(())
}

/// Says where a backup's QR code was saved as a picture. It is said before the code is shown, so that it is
/// on the screen while the code is (in its window, or drawn here after `t`), whichever way that ends.
fn say_picture_saved(picture: &Path) {
    eprintln!("\nThe QR code is saved as a picture too, next to your backups:\n  {}", full_path(picture).display());
    eprintln!("You can print that picture, or open it on your phone or computer.");
}

/// Save the QR code of a backup as a picture (PNG) in the backups folder, next to the
/// backup itself. Never overwrites: if the same picture is already there it is kept as is.
fn save_backup_qr_picture(name: &str, text: &str) -> Res<PathBuf> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)?;
    let picture = qrimg::png(&code, 10);
    let dir = ensure_backup_folder()?;
    let path = dir.join(format!("{name}-qr.png"));
    if path.exists() {
        if fs::read(&path).map(|old| old == picture).unwrap_or(false) {
            return Ok(path);
        }
        let other = unused_path(&dir, &format!("{name}-qr"), "png");
        write_new_file(&other, &picture)?;
        return Ok(other);
    }
    write_new_file(&path, &picture)?;
    Ok(path)
}

// ===========================================================================
// Choosing a password
// ===========================================================================

fn choose_new_password() -> Res<Zeroizing<String>> {
    if !ui::interactive() {
        return ui::secret("");
    }
    eprintln!("\nNow choose a password for the backup file. You will keep it in your password manager.");
    eprintln!("Tip: press Enter on an empty line and SeedLock will generate a strong one for you.\n");
    loop {
        let pw = ui::secret("Password (hidden, empty = generate one): ")?;
        if pw.is_empty() {
            return generate_and_confirm_password();
        }
        let len = pw.chars().count();
        if len < backup::MIN_PASSWORD_CHARS {
            eprintln!("Too short: use at least {} characters.", backup::MIN_PASSWORD_CHARS);
            continue;
        }
        if len < 12 && !ui::confirm("That password is fairly short. Use it anyway?", false)? {
            continue;
        }
        let again = ui::secret("Repeat the password: ")?;
        if *again != *pw {
            eprintln!("The two passwords are different. Let's start again.");
            continue;
        }
        return Ok(pw);
    }
}

/// Generate a word-based password, show it, and only continue once the person
/// has typed it back (copied from their password manager). That proves it was
/// saved correctly before the only place it is displayed disappears.
fn generate_and_confirm_password() -> Res<Zeroizing<String>> {
    let pw = entropy::generate_password(8)?;
    ui::clear_screen();
    eprintln!("Your new backup password (8 random words, about 88 bits of entropy):\n");
    eprintln!("    {}\n", pw.as_str());
    eprintln!("1. Save it in your password manager now, exactly as shown.");
    eprintln!("2. Then type it below (copy it back from the password manager) to prove it was saved correctly.");
    eprintln!("   Without this password the backup can never be opened.\n");
    loop {
        let typed = ui::secret("Type the password (hidden): ")?;
        if *typed == *pw {
            ui::clear_screen();
            eprintln!("Password confirmed.");
            return Ok(pw);
        }
        eprintln!("That does not match. Check the password manager entry and try again.");
    }
}

// ===========================================================================
// Saving a backup
// ===========================================================================

fn finish_backup(m: &Mnemonic, save: &SaveOpts) -> Res<()> {
    let fp = fingerprint_of(m)?;
    let path = new_backup_path(&save.out)?;

    let password = choose_new_password()?;
    let params = save.kdf.params();
    eprintln!("\nEncrypting with Argon2id ({} MiB)...", params.memory_mib());
    let container = backup::encrypt(&mnemonic::entropy(m), &password, params)?;
    write_new_file(&path, backup::to_file_contents(&container).as_bytes())?;

    // Read the file back from disk and decrypt it: proves that what is now on
    // the drive really opens with this password.
    eprintln!("Checking the saved file by opening it again...");
    let saved = fs::read_to_string(&path)?;
    let reopened = backup::decrypt_mnemonic(&saved, &password)?;
    if *mnemonic::entropy(&reopened) != *mnemonic::entropy(m) {
        return fail("the saved file does not decrypt to the same seed. Do NOT rely on it; please try again.");
    }

    eprintln!("\nDone. Your encrypted backup is saved and verified:\n  {}", full_path(&path).display());
    eprintln!("Wallet fingerprint: {fp}");
    eprintln!("\nNext steps:");
    eprintln!("  - Copy that file (or the whole \"{BACKUP_FOLDER}\" folder) to a USB stick and/or your cloud storage.");
    eprintln!("    It is safe there: it is encrypted.");
    eprintln!("  - Keep the password in your password manager, separate from where the file is stored.");
    eprintln!("  - Test the backup now and then: choose \"Check if the backup opens\" in the menu.");

    let want_qr = save.qr
        || (ui::interactive()
            && ui::confirm("\nShow the encrypted backup as a QR code (to scan with a phone)?", false)?);
    if want_qr {
        // the code carries the backup's name, which the phone or computer that scans it suggests
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let code = backup::qr_text(&backup::to_text(&container), &name)?;
        // kept as a picture too, as menu 8 does, and said before the code is shown
        let picture = save_backup_qr_picture(if name.is_empty() { SCRIPT_DEFAULT_NAME } else { &name }, &code)?;
        say_picture_saved(&picture);
        show_qr(&code)?;
    }
    Ok(())
}

// ===========================================================================
// new
// ===========================================================================

pub fn new_seed(args: NewArgs) -> Res<()> {
    let entropy_len = match entropy::entropy_len_for_words(args.words) {
        Some(n) => n,
        None => return fail("--words must be 12, 15, 18, 21 or 24"),
    };
    if ui::interactive() {
        eprintln!("\nYou are about to create a NEW seed. Do this in private: nobody looking, and no camera pointed at the screen.");
        ui::pause("Press Enter to continue... ");
    }
    let raw = if args.dice {
        ask_dice(entropy_len, args.dice_only)?
    } else {
        entropy::generate(entropy_len)?
    };
    let m = mnemonic::from_entropy(&raw)?;
    drop(raw);
    if ui::interactive() {
        show_and_quiz(&m, args.no_quiz)?;
    }
    finish_backup(&m, &args.save)
}

/// Collect dice rolls with a running counter. At a terminal the counter goes up with every
/// roll as it is typed (no Enter needed); when input is piped, rolls are read line by line.
/// Either way a typo costs one roll (or one line), never everything.
fn ask_dice(entropy_len: usize, dice_only: bool) -> Res<Zeroizing<Vec<u8>>> {
    let need = entropy::min_dice_rolls(entropy_len, dice_only);
    let words = entropy_len * 3 / 4;
    eprintln!(
        "\nRoll a six-sided die {} times for a 12-word seed, or {} times for a 24-word seed.",
        entropy::min_dice_rolls(16, dice_only),
        entropy::min_dice_rolls(32, dice_only)
    );
    eprintln!("You chose {words} words, so you need {need} rolls.\n");
    // Live typing wherever single keys can be read (a Unix terminal, a Windows console window).
    let keys = if ui::interactive() { ui::KeyReader::new() } else { None };
    if keys.is_some() {
        eprintln!("Type each result as you roll (digits 1 to 6). The counter goes up with every roll you type.");
        eprintln!("Made a mistake? Press Backspace to remove the last roll. What you type stays hidden.");
    } else {
        eprintln!("Type your results (digits 1 to 6), as many as you like per line, then press Enter.");
        eprintln!("The counter in front of each line shows how many rolls SeedLock has counted so far.");
        eprintln!("Made a mistake? Type u and press Enter to remove the last line. What you type stays hidden.");
    }
    if dice_only {
        eprintln!("Dice-only mode: the same rolls always give the same seed, exactly like SeedSigner.");
    } else {
        eprintln!("Your rolls are mixed with the computer's own randomness, so a few careless rolls cannot hurt.");
    }
    let all = match keys {
        Some(keys) => ask_dice_live(keys, need)?,
        None => ask_dice_lines(need)?,
    };
    Ok(entropy::from_dice(&all, entropy_len, !dice_only)?)
}

/// The rolls one key at a time: the counter changes at the moment a roll is typed.
fn ask_dice_live(mut keys: ui::KeyReader, need: usize) -> Res<Zeroizing<String>> {
    let mut rolls: Zeroizing<String> = Zeroizing::new(String::with_capacity(need));
    let show = |count: usize, note: &str| {
        if count >= need {
            eprint!("\r\x1b[K[{count} of {need} rolls] Done. Press Enter to continue (Backspace changes the last roll).");
        } else {
            eprint!("\r\x1b[K[{count} of {need} rolls] > {note}");
        }
        let _ = std::io::stderr().flush();
    };
    eprintln!();
    show(0, "");
    loop {
        match keys.key() {
            ui::Key::Char(c @ '1'..='6') => {
                if rolls.len() < need {
                    rolls.push(c);
                    show(rolls.len(), "");
                }
            }
            ui::Key::Char(' ') | ui::Key::Char(',') => {}
            ui::Key::Char('u') | ui::Key::Char('U') | ui::Key::Backspace => {
                rolls.pop();
                show(rolls.len(), "");
            }
            ui::Key::Char(_) => show(rolls.len(), "(only the digits 1 to 6 count)"),
            ui::Key::Enter => {
                if rolls.len() >= need {
                    break;
                }
            }
            ui::Key::Cancel => {
                drop(keys); // the terminal is put back first
                eprintln!();
                return fail("cancelled: no seed was created");
            }
            ui::Key::Other => {}
        }
    }
    drop(keys);
    eprintln!("\nThank you!");
    Ok(rolls)
}

/// The rolls line by line (input that is not a terminal, and systems without live keys).
fn ask_dice_lines(need: usize) -> Res<Zeroizing<String>> {
    let mut lines: Vec<Zeroizing<String>> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut total = 0usize;
    while total < need {
        let line = match ui::secret(&format!("\n[{total} of {need} rolls] > ")) {
            Ok(line) => line,
            Err(e) if !ui::interactive() => {
                return fail(format!("only {total} of the {need} dice rolls were provided ({e})"))
            }
            Err(e) => return Err(e),
        };
        let typed = line.trim();
        if typed.eq_ignore_ascii_case("u") || typed.eq_ignore_ascii_case("undo") {
            match (lines.pop(), counts.pop()) {
                (Some(_), Some(n)) => {
                    total -= n;
                    eprintln!("Removed the last line ({n} rolls).");
                }
                _ => eprintln!("There is nothing to undo yet."),
            }
            continue;
        }
        match entropy::validate_rolls(typed) {
            Ok(0) => {}
            Ok(n) => {
                total += n;
                counts.push(n);
                lines.push(Zeroizing::new(typed.to_string()));
            }
            Err(err) if ui::interactive() => eprintln!("{err}. That line was ignored: please type it again."),
            Err(err) => return fail(format!("{err} (you had entered {total} of the {need} rolls needed)")),
        }
    }
    eprintln!("\n[{total} of {need} rolls] Done. Thank you!");
    Ok(Zeroizing::new(lines.iter().map(|l| l.as_str()).collect::<Vec<_>>().join(" ")))
}

fn show_and_quiz(m: &Mnemonic, no_quiz: bool) -> Res<()> {
    let words: Vec<&str> = m.words().collect();
    let fp = fingerprint_of(m)?;
    loop {
        ui::clear_screen();
        eprintln!("Your new seed: {} words. Write them on paper, in this exact order.\n", words.len());
        ui::print_words(&words);
        eprintln!("\nWallet fingerprint: {fp}  (a short ID that lets you recognise this wallet later)");
        ui::pause("\nPress Enter once every word is written down... ");
        ui::clear_screen();
        if no_quiz {
            return Ok(());
        }
        eprintln!("Quick check: type three of your words to make sure they were copied correctly.\n");
        if quiz(&words)? {
            return Ok(());
        }
        eprintln!("\nThat did not match what you wrote. Let's look at the words again.");
        ui::pause("Press Enter... ");
    }
}

fn quiz(words: &[&str]) -> Res<bool> {
    let mut picks: Vec<usize> = Vec::new();
    while picks.len() < 3 {
        let p = random_below(words.len())?;
        if !picks.contains(&p) {
            picks.push(p);
        }
    }
    picks.sort_unstable();
    for p in picks {
        let typed = ui::read_line(&format!("  Word #{}: ", p + 1))?;
        let typed = typed.trim().to_lowercase();
        let ok = typed == words[p]
            || (typed.len() >= 3
                && words[p].starts_with(typed.as_str())
                && mnemonic::LANGUAGE.words_by_prefix(&typed).len() == 1);
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Uniform random number in `0..n` (rejection sampling, no modulo bias).
fn random_below(n: usize) -> Res<usize> {
    let limit = 256 - (256 % n);
    loop {
        let mut b = [0u8; 1];
        entropy::random_bytes(&mut b)?;
        if (b[0] as usize) < limit {
            return Ok(b[0] as usize % n);
        }
    }
}

// ===========================================================================
// encrypt / decrypt / verify
// ===========================================================================

pub fn encrypt(args: EncryptArgs) -> Res<()> {
    if ui::interactive() {
        eprintln!("\nEncrypt a mnemonic you already have into a password-protected backup file.\n");
    }
    let m = read_mnemonic()?;
    let fp = fingerprint_of(&m)?;
    eprintln!("Mnemonic accepted ({} words). Wallet fingerprint: {fp}", m.word_count());
    finish_backup(&m, &args.save)
}

pub fn decrypt(args: DecryptArgs) -> Res<()> {
    let text = load_backup_text(&args.backup)?;
    let m = unlock(&text)?;
    if args.raw || !std::io::stdout().is_terminal() {
        println!("{}", mnemonic::phrase(&m).as_str());
        return Ok(());
    }
    let words: Vec<&str> = m.words().collect();
    let fp = fingerprint_of(&m)?;
    ui::clear_screen();
    eprintln!("Your mnemonic ({} words). Make sure nobody can see your screen.\n", words.len());
    ui::print_words(&words);
    eprintln!("\nWallet fingerprint: {fp}");
    ui::pause("\nPress Enter to clear the screen... ");
    ui::clear_screen();
    Ok(())
}

/// Returns `false` when a requested comparison failed (exit code 2).
pub fn verify(args: VerifyArgs) -> Res<bool> {
    let text = load_backup_text(&args.backup)?;
    let m = unlock(&text)?;
    let fp = fingerprint_of(&m)?;
    eprintln!("Backup OK: it decrypts correctly and the mnemonic is valid.");
    eprintln!("Wallet fingerprint: {fp}");
    println!("{fp}");

    let mut all_ok = true;
    if let Some(expected) = &args.fingerprint {
        if expected.trim().eq_ignore_ascii_case(&fp) {
            eprintln!("The fingerprint matches the one you expected.");
        } else {
            eprintln!("MISMATCH: you expected fingerprint {}, but this backup is {fp}.", expected.trim());
            all_ok = false;
        }
    }
    if args.compare_words {
        eprintln!("\nNow type the words from your paper backup to compare them.");
        let typed = read_mnemonic()?;
        if *mnemonic::entropy(&typed) == *mnemonic::entropy(&m) {
            eprintln!("The words you typed MATCH this backup.");
        } else {
            eprintln!("MISMATCH: the words you typed are a different seed than this backup.");
            all_ok = false;
        }
    }
    Ok(all_ok)
}

// ===========================================================================
// info
// ===========================================================================

pub fn info(args: InfoArgs) -> Res<()> {
    let text = load_backup_text(&args.backup)?;
    let m = unlock(&text)?;
    let passphrase = ask_passphrase(args.passphrase)?;
    let network = args.network.network();
    let script = args.script.script();
    let wallet = Wallet::new(&m, &passphrase, network)?;
    drop(passphrase);

    let account_path = wallet.account_path(script, args.account)?;
    let base = path_to_display(&account_path);
    println!("Wallet fingerprint : {}", wallet.fingerprint());
    println!("Network            : {}", network_name(network));
    println!("Address type       : {}", script.label());
    println!("Account            : {} ({base})", args.account);
    if args.passphrase {
        println!("BIP39 passphrase   : in use (this is a different wallet from the one without it)");
    }
    println!("\nFor Sparrow (File > New Wallet, Keystore \"Watch Only Wallet\"):");
    println!("  Master fingerprint : {}", wallet.fingerprint());
    println!("  Derivation         : {}", path_to_display(&account_path));
    println!("  xpub               : {}", wallet.account_xpub(script, args.account)?);
    println!("\nAccount xpub with key origin, in descriptor notation (for wallets that ask for it in one piece):");
    println!("  {}", wallet.xpub_with_origin(script, args.account)?);
    println!("\nOutput descriptors:");
    println!("  receive: {}", wallet.descriptor(script, args.account, false)?);
    println!("  change : {}", wallet.descriptor(script, args.account, true)?);

    let chain = u32::from(args.change);
    let chain_name = if args.change { "change" } else { "receive" };
    println!("\n{} {chain_name} addresses starting at index {} (compare with your wallet):", args.count, args.start);
    for i in args.start..args.start.saturating_add(args.count) {
        let address = wallet.address(script, args.account, args.change, i)?;
        let path = format!("{base}/{chain}/{i}");
        println!("  {path:<22} {address}");
    }
    Ok(())
}

// ===========================================================================
// sign
// ===========================================================================

enum QrChoice {
    Animated,
    Static,
    No,
}

pub fn sign(args: SignArgs) -> Res<()> {
    let (tx, input) = acquire_transaction(&args)?;
    sign_transaction(tx, input, &args)
}

fn read_psbt_file(path: &Path) -> Res<Psbt> {
    let meta = fs::metadata(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if !meta.is_file() {
        return fail(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_PSBT_FILE {
        return fail("that file is too large to be a PSBT");
    }
    Ok(psbt::decode(&fs::read(path)?)?)
}

/// Read a backup by scanning its QR code (the one menu 8 makes) with the camera. Returns the
/// backup text, exactly as if it had been pasted; nothing is saved.
fn scan_backup(camera: u32) -> Res<String> {
    eprintln!("\nOpening the camera ...");
    let mut cam = scan::Camera::open(camera, true)?;
    match &cam.preview_note {
        None => eprintln!("A window shows what the camera sees. Hold the backup's QR code in front of the camera, big and sharp."),
        Some(why) => eprintln!("(There is no preview window: {why}. Scanning works without it.)"),
    }
    eprintln!("Show the backup's QR code (generated in menu 8, or printed or on a phone). Press Ctrl+C to give up.\n");
    let text = scan::scan_backup_text(&mut cam, scan::GIVE_UP_AFTER)?;
    drop(cam); // the camera is switched off as soon as the code is read
    Ok(text)
}

/// Scan the transaction from the QR code that another wallet shows, using the camera.
fn scan_transaction(camera: u32) -> Res<Psbt> {
    eprintln!("\nOpening the camera ...");
    let mut cam = scan::Camera::open(camera, true)?;
    match &cam.preview_note {
        None => {
            eprintln!("A window shows what the camera sees. Hold the camera so that the whole QR code is inside that window,");
            eprintln!("big and sharp. A frame appears around a QR code that was read, and the bar at the bottom fills as the transaction arrives.");
        }
        Some(why) => eprintln!("(There is no preview window: {why}. Scanning works without it.)"),
    }
    eprintln!("\nIn Sparrow, show the transaction as a QR code, and hold this computer's camera in front of that screen.");
    eprintln!("The code is animated and changes all the time: that is normal. Press Ctrl+C to give up.\n");
    let tx = scan::scan_psbt(&mut cam, scan::GIVE_UP_AFTER)?;
    drop(cam); // the camera is switched off as soon as the transaction is complete
    Ok(tx)
}

/// The transaction to sign, and the file it came from (none when it was scanned).
fn acquire_transaction(args: &SignArgs) -> Res<(Psbt, Option<PathBuf>)> {
    match &args.psbt {
        Some(path) => Ok((read_psbt_file(path)?, Some(path.clone()))),
        None => Ok((scan_transaction(args.camera)?, None)),
    }
}

fn sign_transaction(mut tx: Psbt, input: Option<PathBuf>, args: &SignArgs) -> Res<()> {
    let registered = load_multisig_wallets();
    let (network, network_label) = choose_network(&tx, args.network, &registered)?;
    let text = load_backup_text(&args.backup)?;
    let m = unlock(&text)?;
    let passphrase = ask_passphrase(args.passphrase)?;
    let wallet = Wallet::new(&m, &passphrase, network.network())?;
    drop(passphrase);
    eprintln!("\nWallet fingerprint: {}    Network: {network_label}", wallet.fingerprint());

    let review = psbt::review_with(&tx, &wallet, &registered)?;
    print_review(&review);

    if !args.yes && !ui::confirm("Do you want to SIGN this transaction?", false)? {
        eprintln!("Cancelled. Nothing was signed.");
        return Ok(());
    }
    let signed = psbt::sign_with(&mut tx, &wallet, &registered)?;
    drop(wallet);
    drop(m); // the seed is no longer needed: nothing below touches it
    let out = save_signed(&tx, args, &input)?;
    if !std::io::stdout().is_terminal() {
        println!("{}", psbt::encode_base64(&tx)); // for scripts; a person does not need a wall of text
    }
    eprintln!("\nSigned {signed} input(s). The signed PSBT is saved as:\n  {}", full_path(&out).display());

    let choice = if args.static_qr {
        QrChoice::Static
    } else if args.qr || args.qr_terminal || args.qr_browser {
        QrChoice::Animated
    } else if ui::interactive() {
        ask_qr_choice()?
    } else {
        QrChoice::No
    };
    let place = if args.qr_browser {
        QrPlace::Browser
    } else if args.qr_terminal {
        QrPlace::Terminal
    } else {
        QrPlace::Window
    };
    match choice {
        QrChoice::Animated => show_signed_qr(&tx, &out, false, place)?,
        QrChoice::Static => show_signed_qr(&tx, &out, true, place)?,
        QrChoice::No => eprintln!("Take that file to your online wallet to finish and broadcast the transaction."),
    }
    Ok(())
}

/// Write the signed PSBT: where `--out` says, else next to the input file (a USB stick
/// stays a USB stick), else in the transactions folder. Never overwrites anything.
fn save_signed(tx: &Psbt, args: &SignArgs, input: &Option<PathBuf>) -> Res<PathBuf> {
    let bytes = tx.serialize();
    if let Some(out) = &args.out {
        write_new_file(out, &bytes)?;
        return Ok(out.clone());
    }
    if let Some(input) = input {
        let beside = default_signed_path(input);
        match write_new_file(&beside, &bytes) {
            Ok(()) => return Ok(beside),
            Err(e) => eprintln!("Could not save next to the transaction file ({e}). Using the transactions folder instead."),
        }
    }
    let dir = ensure_tx_folder()?;
    let id = tx.unsigned_tx.compute_txid().to_string();
    let path = unused_path(&dir, &format!("signed-{}", &id[..8]), "psbt");
    write_new_file(&path, &bytes)?;
    Ok(path)
}

fn ask_qr_choice() -> Res<QrChoice> {
    eprintln!("\nHow do you want to give the signed transaction back to Sparrow?");
    eprintln!("  1) Show it as a QR code in a window of its own, for Sparrow to scan with its camera (no USB stick needed)");
    eprintln!("  2) I will copy the saved file myself (for example on a USB stick)");
    loop {
        match ui::read_line("Choose 1 or 2 [1]: ")?.trim() {
            "" | "1" => return Ok(QrChoice::Animated),
            "2" => return Ok(QrChoice::No),
            _ => eprintln!("Please type 1 or 2."),
        }
    }
}

/// Where the signed transaction's QR code is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum QrPlace {
    /// A window of its own (the default); the terminal if no window can be opened.
    Window,
    Terminal,
    Browser,
}

/// SEEDLOCK_NO_QR_WINDOW switches the QR window off (the terminal is used instead).
fn qr_window_wanted() -> bool {
    std::env::var_os("SEEDLOCK_NO_QR_WINDOW").is_none()
}

/// Show QR codes in a window of their own, like Sparrow does, so that their size does not depend
/// on this terminal. `Ok(true)`: shown until the person pressed Enter; `Ok(false)`: they typed t
/// to have the code drawn in the terminal instead; `Err`: no window could be opened (says why).
fn show_in_window(title: &str, texts: &[String], level: EcLevel, how_to_scan: &str) -> Result<bool, String> {
    let codes = ui::same_size_codes(texts, level).map_err(|e| e.to_string())?;
    let window = qrwindow::QrWindow::open(title, &codes, 300)?;
    let changing = if texts.len() > 1 { " (it changes all the time: that is normal)" } else { "" };
    eprintln!("\nThe QR code is shown in a window of its own{changing}.");
    eprintln!("{how_to_scan}");
    eprintln!("The window can be made bigger by dragging its corner. If you do not see it, it may be behind this one.");
    let answer = ui::read_line("When done, click on this terminal and press Enter (or type t to draw the QR code here instead): ");
    drop(window);
    Ok(!matches!(answer, Ok(ref typed) if typed.trim().eq_ignore_ascii_case("t")))
}

/// Show the signed transaction as a QR code that another wallet can scan: in a window of its own
/// (the default), drawn in this terminal, or as a large web page in the browser. The animated form
/// is the UR format SeedSigner uses.
fn show_signed_qr(tx: &Psbt, signed: &Path, is_static: bool, place: QrPlace) -> Res<()> {
    let plain: Vec<String> = if is_static { vec![psbt::encode_base64(tx)] } else { psbt::ur_frames(tx)? };
    if !ui::can_animate() {
        // Nobody to show it to: print it as text for other tools to draw.
        if is_static {
            return show_static_terminal(&plain[0]);
        }
        for frame in &plain {
            println!("{frame}");
        }
        return Ok(());
    }
    // Upper case lets UR codes use the compact alphanumeric QR mode (base64 is case-sensitive).
    let texts: Vec<String> = if is_static { plain } else { plain.iter().map(|f| f.to_uppercase()).collect() };
    match place {
        QrPlace::Browser => match show_in_browser(&texts, signed, is_static) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => eprintln!("(Could not prepare the web page: {e})"),
        },
        QrPlace::Window if qr_window_wanted() => {
            let how = "In Sparrow, choose the option to scan a QR code with the camera, and hold that camera toward the window.";
            match show_in_window("SeedLock - scan this QR code with Sparrow", &texts, EcLevel::L, how) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(why) => eprintln!("\n(No QR window: {why}. The QR code is drawn in this terminal instead.)"),
            }
        }
        QrPlace::Window | QrPlace::Terminal => {}
    }
    if is_static {
        show_static_terminal(&texts[0])
    } else {
        show_animated_terminal(&texts)
    }
}

/// Write a web page with the QR code(s) next to the signed file and open it in the browser.
/// Returns false if no browser could be started (the caller then draws it in the terminal).
fn show_in_browser(texts: &[String], signed: &Path, is_static: bool) -> Res<bool> {
    let codes = ui::same_size_codes(texts, EcLevel::L)?;
    let scan_hint = "In Sparrow, choose the option to scan a QR code with the camera, then hold that camera toward this screen.";
    let hints: [&str; 3] = if is_static {
        [scan_hint, "This is one single, dense code. If Sparrow cannot read it, use the animated code instead.", "Tip: press F11 to fill the whole screen. The bigger the code, the easier it is to read."]
    } else {
        [scan_hint, "The picture changes every moment: that is normal. Keep the camera on it until Sparrow says it has the whole transaction.", "Tip: press F11 to fill the whole screen. The bigger the code, the easier it is to read."]
    };
    let page = qrimg::page("SeedLock: signed transaction", "Scan this QR code with Sparrow", &hints, &codes, 250);
    // Always in the transactions folder, never next to the input file: that may be on a USB stick, which
    // a sandboxed browser (the "snap" Firefox on Ubuntu, for example) is often not allowed to read.
    let dir = ensure_tx_folder()?;
    let stem = signed.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "signed".to_string());
    let path = unused_path(&dir, &format!("{stem}.qr"), "html");
    write_new_file(&path, page.as_bytes())?;
    let shown = full_path(&path);
    if !ui::open_with_default_app(&shown) {
        eprintln!("\nSeedLock could not open a web browser by itself. The page with the QR code is saved here (double-click it to open it):");
        eprintln!("  {}", shown.display());
        if cfg!(target_os = "linux") {
            eprintln!("(On Linux SeedLock starts your browser with the program xdg-open, from the package xdg-utils; on Arch: sudo pacman -S xdg-utils.");
            eprintln!(" It also needs a default browser, for example:  xdg-settings set default-web-browser firefox.desktop )");
        }
        eprintln!("The QR code is also drawn below, in this window.\n");
        return Ok(false);
    }
    eprintln!("\nThe QR code is opening in your web browser. It works offline: nothing is sent anywhere.");
    eprintln!("In Sparrow, choose the option to scan a QR code with the camera and hold that camera toward this screen.");
    eprintln!("The picture changes every moment: that is normal. Tip: press F11 to fill the whole screen.");
    eprintln!("If no browser window appeared, open this file yourself (double-click it):\n  {}", shown.display());
    let answer = ui::read_line("\nPress Enter here when Sparrow has finished scanning (or type t to draw the QR code in this window instead): ")?;
    if answer.trim().eq_ignore_ascii_case("t") {
        return Ok(false); // the browser showed nothing useful: draw it here
    }
    Ok(true)
}

/// In Apple's own Terminal app, explain (boxed, so it cannot be lost among the surrounding text)
/// why the QR code is drawn so much larger there and what to do if it does not fit.
fn apple_terminal_note() {
    if ui::is_apple_terminal() && ui::qr_style() == ui::QrStyle::Full {
        ui::boxed_note(
            "IMPORTANT NOTE",
            &[
                "This is Apple's built-in Terminal app. It draws the characters that SeedLock normally uses for QR \
                 codes a little too short, which leaves a thin white line through the black parts at the end of \
                 every line of text. The code still looks fine, but a camera cannot read it.",
                "So here SeedLock draws a larger code without those characters. It needs a big window: maximize \
                 this window, then zoom out with Cmd and - until the whole code fits.",
                "If the code still cannot be read, iTerm2 (free, from https://iterm2.com) worked for a SeedLock \
                 user on a Mac. For a transaction, the sign command's --qr-browser option shows the code in your web \
                 browser instead.",
            ],
        );
    }
}

/// The animated QR code drawn in the terminal (used when no browser can be opened).
fn show_animated_terminal(texts: &[String]) -> Res<()> {
    apple_terminal_note();
    let art = ui::qr_frames_terminal(texts, EcLevel::L)?;
    let total = art.len();
    let blocks: Vec<String> = art
        .iter()
        .enumerate()
        .map(|(i, a)| format!("{a}Frame {}/{total} - press Enter when done\x1b[K\n", i + 1))
        .collect();
    let (lines, columns) = ui::block_size(&blocks[0]);
    if !ui::wait_for_room(lines + 4, columns + 1, "animated QR code")? {
        return Ok(());
    }
    eprintln!("\nPoint the camera of the other device at this QR code (in Sparrow, use its option to scan a transaction from a QR code).");
    eprintln!(
        "If the code looks blurry, zoom the text ({} and +). Press Enter here when the other wallet has finished scanning.\n",
        ui::zoom_key()
    );
    ui::animate(&blocks, 300);
    Ok(())
}

/// One static QR code (base64 text) drawn in the terminal. Only small transactions fit.
fn show_static_terminal(text: &str) -> Res<()> {
    apple_terminal_note();
    let art = match ui::qr_terminal(text, EcLevel::L) {
        Ok(art) => art,
        Err(_) => {
            return fail("this transaction is too big for one QR code. Use the animated QR code instead (--qr instead of --static-qr).")
        }
    };
    let (lines, columns) = ui::block_size(&art);
    if !ui::wait_for_room(lines + 4, columns + 1, "QR code")? {
        return Ok(());
    }
    eprintln!("\n{art}");
    if text.len() > 350 {
        eprintln!("Note: this QR code is dense and can be hard to scan. If it does not work, use the animated QR code.");
    }
    Ok(())
}

/// `tx.psbt` -> `tx.signed.psbt`, or `tx.signed-2.psbt`, ... if that name is taken.
fn default_signed_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "transaction".to_string());
    let first = input.with_file_name(format!("{stem}.signed.psbt"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| input.with_file_name(format!("{stem}.signed-{n}.psbt")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

fn print_review(r: &psbt::Review) {
    eprintln!("\n================ TRANSACTION REVIEW ({}) ================", network_name(r.network));
    eprintln!("\nInputs ({}):", r.inputs.len());
    for i in &r.inputs {
        let owner = match (&i.path, &i.multisig) {
            (Some(p), Some(wallet)) => format!("yours, from the multisig wallet \"{wallet}\" ({p})"),
            (Some(p), None) => format!("yours ({p})"),
            (None, _) => "NOT yours".to_string(),
        };
        eprintln!("  #{}  {} BTC  {owner}", i.index, psbt::format_btc(i.value));
    }
    eprintln!("\nOutputs ({}):", r.outputs.len());
    for o in &r.outputs {
        eprintln!("  #{}  {} BTC", o.index, psbt::format_btc(o.value));
        eprintln!("      to {}", o.address);
        match &o.kind {
            OutputKind::External => {
                eprintln!("      >>> SENT TO SOMEONE ELSE. Check this address carefully! <<<")
            }
            OutputKind::Change { path } => {
                eprintln!("      change back to you (verified from your seed: {path})")
            }
            OutputKind::OwnReceive { path } => {
                eprintln!("      goes to your own address (verified from your seed: {path})")
            }
            OutputKind::MultisigChange { path, wallet } => {
                eprintln!("      change back to the multisig wallet \"{wallet}\" (verified from all its cosigners' keys: {path})")
            }
            OutputKind::MultisigReceive { path, wallet } => {
                eprintln!("      goes to the multisig wallet \"{wallet}\" (verified from all its cosigners' keys: {path})")
            }
        }
    }
    eprintln!("\nTotal in : {} BTC", psbt::format_btc(r.total_in));
    eprintln!("Total out: {} BTC", psbt::format_btc(r.total_out));
    eprintln!("Fee      : {} BTC", psbt::format_btc(r.fee));
    eprintln!("Leaving your wallet: {} BTC", psbt::format_btc(r.sending));
    if let Some(m) = &r.multisig {
        let after = m.signed_by_others + 1;
        eprintln!(
            "\nMultisig \"{}\", {} of {}: this seed is cosigner {} and adds its signature ({} of the {} needed, with it).",
            m.name, m.threshold, m.cosigners, m.position, after.min(m.threshold), m.threshold
        );
        if after >= m.threshold {
            eprintln!("That is enough: after this, Sparrow can finalize and broadcast the transaction.");
        } else {
            let more = m.threshold - after;
            eprintln!("{more} more cosigner{} must sign it too before Sparrow can finalize it.", if more == 1 { "" } else { "s" });
        }
    }
    if !r.warnings.is_empty() {
        eprintln!("\nWARNINGS:");
        for w in &r.warnings {
            eprintln!("  ! {w}");
        }
    }
    eprintln!("\n=========================================================\n");
}

// ===========================================================================
// inspect / qr / password / final-word
// ===========================================================================

pub fn inspect(args: InspectArgs) -> Res<()> {
    let text = load_backup_text(&args.backup)?;
    let info = backup::inspect_text(&text)?;
    println!("SeedLock backup, format version {}", info.version);
    println!("Cipher         : XChaCha20-Poly1305");
    println!("Key derivation : Argon2id v1.3");
    println!("  memory       : {} MiB", info.kdf.memory_mib());
    println!("  passes       : {}", info.kdf.t_cost);
    println!("  parallelism  : {}", info.kdf.parallelism);
    println!("\nOpening it needs the password and about {} MiB of free memory.", info.kdf.memory_mib());
    Ok(())
}

pub fn qr(args: QrArgs) -> Res<()> {
    qr_backup(args, false)
}

/// Copy a saved picture into a folder the person chooses (a USB stick, for example). An existing
/// file there is never replaced.
fn offer_copy(file: &Path) -> Res<()> {
    // named again: a QR code drawn in the terminal may have pushed the message about the picture out of sight
    let shown = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    loop {
        let typed = clean_path(&ui::read_line(&format!(
            "\nSave a copy of {shown} somewhere else too, for example on a USB stick? Type or drop a folder, or press Enter to skip: "
        ))?);
        if typed.is_empty() {
            return Ok(());
        }
        let dir = PathBuf::from(ui::expand_home(&typed));
        if !dir.is_dir() {
            eprintln!("There is no folder at \"{typed}\". Check it and try again, or press Enter to skip.");
            continue;
        }
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "qr".to_string());
        let ext = file.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "png".to_string());
        let target = unused_path(&dir, &stem, &ext);
        match fs::copy(file, &target) {
            Ok(_) => {
                eprintln!("A copy was saved as:\n  {}", full_path(&target).display());
                return Ok(());
            }
            Err(e) => eprintln!("It could not be saved there ({e}). Try another folder, or press Enter to skip."),
        }
    }
}

fn qr_backup(args: QrArgs, ask_for_copy: bool) -> Res<()> {
    let arg = args.backup.trim();
    let text = load_backup_text(arg)?;
    // The code carries the backup's name (its file name, or the one a pasted backup carries), so that the
    // phone or computer that scans it can suggest it; a backup without a name gets a code without one.
    let carried = if arg.to_uppercase().starts_with(backup::TEXT_PREFIX) {
        backup::name_in(&text)
    } else {
        find_backup_file(arg)?.file_stem().map(|s| s.to_string_lossy().into_owned())
    };
    let code = backup::qr_text(&text, carried.as_deref().unwrap_or(""))?;
    let name = carried.clone().unwrap_or_else(|| SCRIPT_DEFAULT_NAME.to_string());
    let picture = save_backup_qr_picture(&name, &code)?;
    say_picture_saved(&picture);
    show_qr(&code)?;
    if ask_for_copy && ui::interactive() {
        offer_copy(&picture)?;
    }
    if let Some(path) = &args.svg {
        write_new_file(path, ui::qr_svg(&code)?.as_bytes())?;
        eprintln!("QR code (SVG) written to {}", full_path(path).display());
    }
    match &carried {
        Some(name) => eprintln!("This QR code holds the ENCRYPTED backup and its name, \"{name}\". It is useless without the password."),
        None => eprintln!("This QR code holds the ENCRYPTED backup only. It is useless without the password."),
    }
    Ok(())
}

pub fn camera(args: CameraArgs) -> Res<()> {
    scan::camera_test(args.camera)
}

pub fn password(args: PasswordArgs) -> Res<()> {
    if !(4..=16).contains(&args.words) {
        return fail("--words must be between 4 and 16");
    }
    let pw = entropy::generate_password(args.words)?;
    println!("{}", pw.as_str());
    eprintln!(
        "({} random words, about {} bits of entropy. Store it in your password manager.)",
        args.words,
        args.words * entropy::PASSWORD_BITS_PER_WORD
    );
    Ok(())
}

pub fn final_word() -> Res<()> {
    eprintln!("Type the first 11 words (or 14, 17, 20, 23 for longer seeds). What you type is hidden.");
    let input = ui::secret("> ")?;
    let candidates = mnemonic::final_word_candidates(&input)?;
    println!("{} valid final words:", candidates.len());
    for row in candidates.chunks(6) {
        println!("  {}", row.join("  "));
    }
    Ok(())
}

// ===========================================================================
// Interactive menu
// ===========================================================================

/// The menu's title, shown first (before SeedLock checks that the computer is offline).
pub fn menu_title() {
    // The version comes from Cargo.toml ([workspace.package] version), the one place it is set.
    eprintln!("\n=== SeedLock {}: offline Bitcoin seed tool ===", env!("CARGO_PKG_VERSION"));
    eprintln!("It works only on a computer that is not connected to any network.");
}

/// The menu. SeedLock got here only because the computer is offline (offline.rs), and it stops at once
/// should that change.
pub fn menu() -> Res<()> {
    eprintln!("\nThis computer is offline: no network connection was found.");
    loop {
        eprintln!("\nWhat would you like to do?");
        eprintln!("  1) Create a new seed and an encrypted backup");
        eprintln!("  2) Encrypt a seed I already have (words -> backup file)");
        eprintln!("  3) Decrypt a backup (show the words)");
        eprintln!("  4) Check if the backup opens (verify)");
        eprintln!("  5) Wallet info: xpub, descriptors, addresses");
        eprintln!("  6) Sign a transaction (PSBT)");
        eprintln!("  7) Generate a strong random password");
        eprintln!("  8) Generate a backup as a QR code");
        eprintln!("  9) Test the camera (used to scan transactions and backups)");
        eprintln!(" 10) Multisig wallets (for example 2 of 3): share this seed's key, register a wallet");
        eprintln!("  q) Quit");
        let choice = match ui::read_line("\n> ") {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let result = match choice.trim().to_lowercase().as_str() {
            "1" => menu_new(),
            "2" => encrypt(EncryptArgs { save: menu_save_opts() }),
            "3" => menu_decrypt(),
            "4" => menu_verify(),
            "5" => menu_info(),
            "6" => menu_sign(),
            "7" => password(PasswordArgs { words: 8 }),
            "8" => menu_qr(),
            "9" => camera(CameraArgs { camera: 0 }),
            "10" => menu_multisig(),
            "q" | "quit" | "exit" => return Ok(()),
            "" => continue,
            _ => {
                eprintln!("Please choose one of the numbers above.");
                continue;
            }
        };
        if let Err(e) = result {
            eprintln!("\nError: {e}");
        }
    }
}

fn menu_save_opts() -> SaveOpts {
    SaveOpts { out: None, kdf: KdfPreset::Standard, qr: false }
}

/// What a person typed or dragged into the window as a path: quotes and (on macOS and
/// Linux) backslashes before spaces are removed.
/// A path as typed, pasted, or dropped onto the terminal window. Dropping a file inserts its path
/// the way each terminal likes: in quotes (most Linux terminals, Windows), with a backslash in
/// front of spaces and other special characters (Apple's Terminal, iTerm2), or as a file:// address
/// with %20 for spaces (some other terminals). All of these give back the plain path.
fn clean_path(input: &str) -> String {
    let path = input.trim().trim_matches('"').trim_matches('\'');
    if let Some(rest) = path.strip_prefix("file://") {
        let decoded = percent_decode(rest.strip_prefix("localhost").unwrap_or(rest));
        // file:///C:/Users/... on Windows
        let windows_drive = decoded.len() > 2 && decoded.starts_with('/') && decoded.as_bytes()[2] == b':';
        return if cfg!(windows) && windows_drive { decoded[1..].to_string() } else { decoded };
    }
    if cfg!(windows) {
        return path.to_string();
    }
    // `\ ` and the like stand for the character after the backslash. Only the characters that a
    // terminal escapes when a file is dropped are undone, so a path such as C:\Users\me keeps its
    // backslashes.
    const ESCAPED: &str = " ()[]{}'\"&;!$*?<>|#~`\\";
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next) if c == '\\' && ESCAPED.contains(next) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// `%20` and the like back to the characters they stand for (in file:// addresses).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(high * 16 + low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Let the person pick one of the backups in the folder by number, type or drop a name or path,
/// or paste the backup text; with `camera`, also scan the backup's QR code (the one menu 8
/// makes). Returns a name, a path, or the backup text itself.
fn pick_backup(question: &str, camera: bool) -> Res<String> {
    eprintln!("\n{question}");
    let backups = list_backups();
    let dir = backup_folder()?;
    let wants_camera = |typed: &str| camera && typed.eq_ignore_ascii_case("c");
    if backups.is_empty() {
        eprintln!("\nThere are no backups in the folder:\n  {}", dir.display());
        eprintln!("(SeedLock looks for that folder in the directory it was started from.)");
        let prompt = if camera {
            "Type the path of a .seedlock file (or drop the file here), paste the SEEDLOCK1:... text, or type c to scan the backup's QR code with the camera: "
        } else {
            "Type the path of a .seedlock file (or drop the file here), or paste the SEEDLOCK1:... text: "
        };
        loop {
            let typed = clean_path(&ui::read_line(prompt)?);
            if wants_camera(&typed) {
                return scan_backup(0);
            }
            if !typed.is_empty() {
                return Ok(typed);
            }
        }
    }
    eprintln!("\nYour backups (folder: {}):", dir.display());
    for (i, (name, _)) in backups.iter().enumerate() {
        eprintln!("  {}) {name}", i + 1);
    }
    if camera {
        eprintln!("  c) Scan a backup's QR code with the camera (generated in menu 8, or printed or on a phone)");
    }
    let only_one = backups.len() == 1;
    let prompt = if only_one {
        "Press Enter to use it, type another name or path, or drop your backup file and press \"Enter\": ".to_string()
    } else {
        format!("Choose a backup: number 1-{}, type a name or path, or drop your backup file and press \"Enter\": ", backups.len())
    };
    loop {
        let typed = clean_path(&ui::read_line(&format!("\n{prompt}"))?);
        if wants_camera(&typed) {
            return scan_backup(0);
        }
        if typed.is_empty() {
            if only_one {
                return Ok(backups[0].1.to_string_lossy().into_owned());
            }
            continue;
        }
        if let Ok(n) = typed.parse::<usize>() {
            if (1..=backups.len()).contains(&n) {
                return Ok(backups[n - 1].1.to_string_lossy().into_owned());
            }
        }
        return Ok(typed);
    }
}

fn ask_network(default: NetworkArg) -> Res<NetworkArg> {
    let name = match default {
        NetworkArg::Mainnet => "mainnet",
        NetworkArg::Testnet => "testnet",
        NetworkArg::Signet => "signet",
        NetworkArg::Regtest => "regtest",
    };
    let a = ui::read_line(&format!("Network: mainnet, testnet, signet or regtest [{name}]: "))?;
    match a.trim().to_lowercase().as_str() {
        "" => Ok(default),
        "mainnet" | "main" => Ok(NetworkArg::Mainnet),
        "testnet" | "test" => Ok(NetworkArg::Testnet),
        "signet" => Ok(NetworkArg::Signet),
        "regtest" => Ok(NetworkArg::Regtest),
        other => fail(format!("unknown network '{other}'")),
    }
}

/// The network to sign for: the one asked for with --network, else the one the transaction's key
/// paths say (every key path in it, m/84'/0'/... being mainnet and m/84'/1'/... a test network, and the
/// version of the wallet keys it may carry, xpub or tpub). Asks only when the transaction cannot tell.
/// Returns it and how to describe it.
fn choose_network(tx: &Psbt, asked: Option<NetworkArg>, registered: &[(String, MultisigWallet)]) -> Res<(NetworkArg, String)> {
    use psbt::NetworkHint;
    if let Some(network) = asked {
        return Ok((network, network_name(network.network()).to_string()));
    }
    let why = match psbt::network_hint(tx) {
        NetworkHint::Mainnet => return Ok((NetworkArg::Mainnet, "mainnet (real bitcoin), found from the transaction".to_string())),
        NetworkHint::TestNetwork => {
            return Ok((NetworkArg::Testnet, "a test network (testnet or signet), found from the transaction".to_string()))
        }
        NetworkHint::Unknown => {
            // legacy multisig key paths (m/45'/...) name no network: a registered wallet that owns an input does
            let owner = tx.inputs.iter().flat_map(|i| i.bip32_derivation.values()).find_map(|(fp, path)| {
                registered.iter().find(|(_, ms)| ms.locate(*fp, path).is_some()).map(|(_, ms)| ms.is_mainnet())
            });
            match owner {
                Some(true) => return Ok((NetworkArg::Mainnet, "mainnet (real bitcoin), from the registered multisig wallet".to_string())),
                Some(false) => {
                    return Ok((NetworkArg::Testnet, "a test network (testnet or signet), from the registered multisig wallet".to_string()))
                }
                None => "This transaction has no key paths that say which network it is for.",
            }
        }
        NetworkHint::Conflicting => "Some key paths in this transaction say mainnet and others a test network: something is wrong with it.",
    };
    if !ui::interactive() {
        return fail(format!("{why} Say which network it is for with --network."));
    }
    eprintln!("\n{why}");
    let network = ask_network(NetworkArg::Mainnet)?;
    Ok((network, network_name(network.network()).to_string()))
}

fn menu_new() -> Res<()> {
    let w = ui::read_line("Number of words, 12 or 24 [24]: ")?;
    let words = match w.trim() {
        "" | "24" => 24,
        "12" => 12,
        other => return fail(format!("choose 12 or 24, not '{other}'")),
    };
    let dice = ui::confirm("Add your own dice rolls as extra randomness?", false)?;
    new_seed(NewArgs { words, dice, dice_only: false, no_quiz: false, save: menu_save_opts() })
}

fn menu_decrypt() -> Res<()> {
    decrypt(DecryptArgs { backup: pick_backup("Which backup do you want to open?", true)?, raw: false })
}

fn menu_verify() -> Res<()> {
    let backup = pick_backup("Which backup do you want to check?", true)?;
    let compare_words = ui::confirm("Also compare it with the words on your paper backup?", false)?;
    if !verify(VerifyArgs { backup, fingerprint: None, compare_words })? {
        eprintln!("\nThe comparison FAILED. Do not rely on this backup until you understand why.");
    }
    Ok(())
}

fn menu_info() -> Res<()> {
    let backup = pick_backup("Which wallet (backup) do you want to see?", true)?;
    let name = named_backup(&backup)?;
    let passphrase_used = ui::confirm("Does this wallet use a BIP39 passphrase?", false)?;
    let text = load_backup_text(&backup)?;
    let m = unlock(&text)?;
    let passphrase = ask_passphrase(passphrase_used)?;
    let fingerprint = Wallet::new(&m, &passphrase, Network::Bitcoin)?.fingerprint();
    let (network, script, found) = match learn_from_transactions(fingerprint) {
        Some(learned) => (
            learned.network,
            learned.script,
            format!("network and address type found from this wallet's latest transaction, {}", learned.file),
        ),
        None => {
            eprintln!("\nThere is no transaction of this wallet here, so SeedLock cannot tell its network by itself.");
            let network = ask_real_or_test()?;
            (network, ScriptType::Wpkh, "Native Segwit, the usual address type; the others are listed below".to_string())
        }
    };
    let wallet = Wallet::new(&m, &passphrase, network.network())?;
    drop(passphrase);
    wallet_for_sparrow(&wallet, script, network, passphrase_used, &found, &name)
}

/// What a wallet's newest transaction says about it.
struct Learned {
    network: NetworkArg,
    script: ScriptType,
    file: String,
}

/// The network and address type of the wallet with this fingerprint, from its newest transaction.
fn learn_from_transactions(fingerprint: seedlock_core::bitcoin::bip32::Fingerprint) -> Option<Learned> {
    let (purpose, network, file) = find_in_transactions(fingerprint, |purpose| ScriptType::from_purpose(purpose).is_some())?;
    Some(Learned { network, script: ScriptType::from_purpose(purpose)?, file })
}

/// The newest transaction with a key path of the wallet with this fingerprint whose purpose (the
/// first step: 84 for native SegWit, 48 for multisig...) is `wanted`: the .psbt files in this folder
/// and in "seedlocker transactions" (signed ones included), newest first. Returns the purpose, the
/// network its coin type says, and the file's name. Only key paths of this very wallet count.
fn find_in_transactions(
    fingerprint: seedlock_core::bitcoin::bip32::Fingerprint,
    wanted: impl Fn(u32) -> bool,
) -> Option<(u32, NetworkArg, String)> {
    use seedlock_core::bitcoin::bip32::ChildNumber;
    let mut dirs = vec![PathBuf::from(".")];
    if let Ok(dir) = transactions_folder() {
        dirs.push(dir);
    }
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_file() && meta.len() <= MAX_PSBT_FILE && path.extension().is_some_and(|x| x.eq_ignore_ascii_case("psbt")) {
                files.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), path));
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().take(50) {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(tx) = psbt::decode(&bytes) else { continue };
        let origins = tx
            .inputs
            .iter()
            .flat_map(|i| i.bip32_derivation.values().chain(i.tap_key_origins.values().map(|(_, origin)| origin)))
            .chain(tx.outputs.iter().flat_map(|o| o.bip32_derivation.values().chain(o.tap_key_origins.values().map(|(_, origin)| origin))));
        for (fp, key_path) in origins {
            if *fp != fingerprint {
                continue;
            }
            let steps: &[ChildNumber] = key_path.as_ref();
            let (Some(ChildNumber::Hardened { index: purpose }), Some(ChildNumber::Hardened { index: coin })) = (steps.first(), steps.get(1)) else {
                continue;
            };
            let network = match coin {
                0 => NetworkArg::Mainnet,
                1 => NetworkArg::Testnet,
                _ => continue,
            };
            if wanted(*purpose) {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                return Some((*purpose, network, file));
            }
        }
    }
    None
}

fn ask_real_or_test() -> Res<NetworkArg> {
    loop {
        match ui::read_line("Is it for real bitcoin (mainnet) or for testing (testnet or signet)? Type m or t [m]: ")?.trim().to_lowercase().as_str() {
            "" | "m" | "main" | "mainnet" => return Ok(NetworkArg::Mainnet),
            "t" | "test" | "testnet" | "signet" => return Ok(NetworkArg::Testnet),
            "regtest" => return Ok(NetworkArg::Regtest),
            _ => eprintln!("Please type m (mainnet) or t (testnet or signet)."),
        }
    }
}

// ===========================================================================
// Multisig wallets (menu 10)
// ===========================================================================

/// `<backup>-cosigner.txt`, `<backup>-cosigner-qr.png` and their `-2`, `-3` ... copies.
fn is_cosigner_file(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if !matches!(ext.to_ascii_lowercase().as_str(), "txt" | "png") {
        return false;
    }
    let base = match stem.rsplit_once('-') {
        Some((base, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => base,
        _ => stem,
    };
    base.ends_with("-cosigner") || base.ends_with("-cosigner-qr")
}

/// Earlier versions kept the registered multisig wallets and the cosigner keys next to the backups:
/// move them into their own folders. A file whose name is already taken there stays where it is.
fn tidy_multisig_files() {
    let Ok(dir) = backup_folder() else { return };
    let Ok(entries) = fs::read_dir(&dir) else { return };
    let mut moved = Vec::new();
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = if name.to_ascii_lowercase().ends_with(".multisig") {
            MULTISIG_FOLDER
        } else if is_cosigner_file(&name) {
            COSIGNER_FOLDER
        } else {
            continue;
        };
        let Ok(target_dir) = ensure_private_folder(dir.join(folder)) else { continue };
        let target = target_dir.join(&name);
        if !target.exists() && fs::rename(&path, &target).is_ok() {
            moved.push(format!("{folder}/{name}"));
        }
    }
    if !moved.is_empty() {
        eprintln!("\n(Moved into their own folders in \"{BACKUP_FOLDER}\": {}.)", moved.join(", "));
    }
}

/// The multisig wallets registered here: `<name>.multisig` files in the "multisig wallets" folder,
/// each holding the wallet's output descriptor. A file that cannot be read is named, never skipped silently.
fn load_multisig_wallets() -> Vec<(String, MultisigWallet)> {
    tidy_multisig_files();
    let Ok(dir) = backup_folder().map(|b| b.join(MULTISIG_FOLDER)) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("multisig")))
        .collect();
    files.sort();
    let mut wallets = Vec::new();
    for path in files {
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let read = fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| MultisigWallet::from_descriptor_text(&text).map_err(|e| e.to_string()));
        match read {
            Ok(wallet) => wallets.push((name, wallet)),
            Err(e) => eprintln!("(The registered multisig wallet {} could not be read: {e})", full_path(&path).display()),
        }
    }
    wallets
}

/// "Native Segwit (P2WSH)", and for multi() "... with its keys in a fixed order".
fn wallet_kind(ms: &MultisigWallet) -> String {
    let order = if ms.sorted { "" } else { " with its keys in a fixed order" };
    format!("{}{order}", ms.script.sparrow_name())
}

/// Scan a multisig wallet's descriptor from Sparrow's QR code with the camera.
fn scan_descriptor(camera: u32) -> Res<MultisigWallet> {
    eprintln!("\nIn Sparrow, show the wallet's descriptor as a QR code: the wallet's Settings, Script Policy, the QR code button.");
    eprintln!("Opening the camera ...");
    let mut cam = scan::Camera::open(camera, true)?;
    match &cam.preview_note {
        None => eprintln!("A window shows what the camera sees. Hold the camera so that the whole QR code is inside it."),
        Some(why) => eprintln!("(There is no preview window: {why}. Scanning works without it.)"),
    }
    eprintln!("The code may be animated: keep the camera on it until SeedLock has all its parts. Press Ctrl+C to give up.\n");
    let wallet = scan::scan_descriptor(&mut cam, scan::GIVE_UP_AFTER)?;
    drop(cam);
    Ok(wallet)
}

fn net_words(mainnet: bool) -> &'static str {
    if mainnet { "mainnet (real bitcoin)" } else { "a test network (testnet or signet)" }
}

fn menu_multisig() -> Res<()> {
    eprintln!("\n=== Multisig wallets (for example 2 of 3) ===");
    eprintln!("A multisig wallet needs several seeds to spend: in a 2-of-3 wallet, any 2 of its 3. Sparrow keeps the wallet");
    eprintln!("(watch-only, on the online computer) and each seed signs on its own. SeedLock can hold one of them, or more.");
    eprintln!("\n  1) Share this seed's key, to add it as a cosigner of a multisig wallet in Sparrow");
    eprintln!("  2) Register a multisig wallet, so that SeedLock can check and sign its transactions");
    eprintln!("  3) Show the registered multisig wallets");
    loop {
        match ui::read_line("\nChoose 1, 2 or 3 (or press Enter to go back): ")?.trim() {
            "" | "q" => return Ok(()),
            "1" => return multisig_share_key(),
            "2" => return multisig_register(),
            "3" => return multisig_list(),
            _ => eprintln!("Please type 1, 2 or 3."),
        }
    }
}

/// Which kind of multisig wallet (Sparrow's "Script Type").
fn ask_multisig_script() -> Res<MultisigScript> {
    eprintln!("\nWhich kind of multisig wallet is it (Sparrow's \"Script Type\")?");
    for (i, script) in MultisigScript::ALL.iter().enumerate() {
        eprintln!("  {}) {}{}", i + 1, script.sparrow_name(), if i == 0 { ", the usual one" } else { "" });
    }
    loop {
        match ui::read_line("Choose 1, 2 or 3 [1]: ")?.trim() {
            "" | "1" => return Ok(MultisigScript::Wsh),
            "2" => return Ok(MultisigScript::ShWsh),
            "3" => return Ok(MultisigScript::Sh),
            _ => eprintln!("Please type 1, 2 or 3."),
        }
    }
}

/// This seed's cosigner key, laid out for Sparrow: as the three fields of a watch-only keystore and
/// as the QR code SeedSigner shows (a UR crypto-account), which Sparrow scans with Airgapped
/// Hardware Wallet > SeedSigner. The key path is BIP 48 (native and nested) or BIP 45 (legacy).
fn multisig_share_key() -> Res<()> {
    use std::fmt::Write as _;
    let backup = pick_backup("Which seed (backup) do you want to add as a cosigner?", true)?;
    let name = named_backup(&backup)?;
    let passphrase_used = ui::confirm("Does this wallet use a BIP39 passphrase?", false)?;
    let text = load_backup_text(&backup)?;
    let m = unlock(&text)?;
    let passphrase = ask_passphrase(passphrase_used)?;
    let fingerprint = Wallet::new(&m, &passphrase, Network::Bitcoin)?.fingerprint();
    let known = load_multisig_wallets()
        .into_iter()
        .find_map(|(name, w)| w.position(fingerprint).map(|p| (name, w.script, w.is_mainnet(), w.cosigners[p].path.clone())));
    let (script, mainnet, path) = match known {
        Some((name, script, mainnet, path)) => {
            eprintln!("\n(This seed is a cosigner of the registered multisig wallet \"{name}\": its kind and network are taken from there.)");
            (script, mainnet, path)
        }
        None => {
            let mainnet = match find_in_transactions(fingerprint, |purpose| purpose == 48 || purpose == 45) {
                Some((_, network, file)) => {
                    eprintln!("\n(The network is found from this seed's latest multisig transaction, {file}.)");
                    matches!(network, NetworkArg::Mainnet)
                }
                None => {
                    eprintln!("\nSeedLock cannot tell by itself which network the multisig wallet is for.");
                    matches!(ask_real_or_test()?, NetworkArg::Mainnet)
                }
            };
            let script = ask_multisig_script()?;
            (script, mainnet, script.account_path(mainnet))
        }
    };
    let wallet = Wallet::new(&m, &passphrase, if mainnet { Network::Bitcoin } else { Network::Testnet })?;
    drop(passphrase);
    let xpub = wallet.xpub_at(&path)?;
    let label = multisig::keystore_label(&name, wallet.fingerprint());
    let shortened = if !name.trim().is_empty() && label != name.trim() { " (Sparrow allows 16 characters)" } else { "" };
    let mut t = String::new();
    let _ = writeln!(t, "Seed {} as a cosigner of a multisig wallet: {}, {}", wallet.fingerprint(), script.sparrow_name(), net_words(mainnet));
    let _ = writeln!(t, "\nIn Sparrow: File > New Wallet, Policy Type \"Multi Signature\" (for example 2 of 3), Script Type");
    let _ = writeln!(t, "\"{}\". For this seed's keystore choose \"Watch Only Wallet\", then either click the camera", script.sparrow_name());
    let _ = writeln!(t, "button next to the xpub field (\"Scan a QR code\") and scan this seed's QR code, which fills in the fields");
    let _ = writeln!(t, "and names the keystore \"{label}\"{shortened}, or type:");
    let _ = writeln!(t, "  Master fingerprint : {}", wallet.fingerprint());
    let _ = writeln!(t, "  Derivation         : {}", path_to_display(&path));
    let _ = writeln!(t, "  xpub               : {xpub}");
    let _ = writeln!(t, "\nIn one piece, for wallets that ask for it that way:\n  [{}/{}]{xpub}", wallet.fingerprint(), path_to_string(&path));
    let _ = writeln!(t, "\nWhen every cosigner's keystore is in and the wallet is saved, register it here (menu 10, then 2): with");
    let _ = writeln!(t, "the QR code of its descriptor (Sparrow: the wallet's Settings, Script Policy, the QR code button) or the");
    let _ = writeln!(t, "file (File > Export Wallet > Output Descriptor). Then SeedLock can check and sign its transactions.");
    print!("\n{t}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let code = multisig::cosigner_account_ur(wallet.fingerprint(), &path, &xpub, script, &label);
    // saved before the QR code is offered, so that where they are is said whatever happens next
    let dir = ensure_private_folder(backup_folder()?.join(COSIGNER_FOLDER))?;
    let text_file = unused_path(&dir, &format!("{name}-cosigner"), "txt");
    write_new_file(&text_file, format!("{t}\nAs a QR code (UR crypto-account):\n  {code}\n").as_bytes())?;
    let picture = unused_path(&dir, &format!("{name}-cosigner-qr"), "png");
    write_new_file(&picture, &qrimg::png(&QrCode::with_error_correction_level(code.as_bytes(), EcLevel::M)?, 8))?;
    eprintln!("\nSaved in the \"{COSIGNER_FOLDER}\" folder of your backups (no secrets, but they show this seed's multisig key):");
    eprintln!("  {}\n  {}", full_path(&text_file).display(), full_path(&picture).display());
    if ui::can_animate() && ui::confirm("\nShow this seed's key as a QR code for Sparrow to scan?", true)? {
        let how = "In Sparrow, in this seed's keystore (Watch Only Wallet), click the camera button next to the xpub field and scan this code.";
        show_text_qr("SeedLock - cosigner key for Sparrow", &code, how)?;
    }
    Ok(())
}

/// Register a multisig wallet from the output descriptor Sparrow exports, after the person checked
/// its cosigners and its first address.
fn multisig_register() -> Res<()> {
    use std::fmt::Write as _;
    eprintln!("\nSparrow gives the wallet's descriptor as a QR code (the wallet's Settings, Script Policy, the QR code");
    eprintln!("button) or as a file (File > Export Wallet > Output Descriptor, brought here on a USB stick, for example).");
    eprintln!("It holds no secrets, only the wallet's public keys.");
    let ms = loop {
        let typed = ui::read_line(
            "\nType or drop that file and press Enter, paste the descriptor, or type c to scan Sparrow's QR code with the camera (q to go back): ",
        )?;
        let typed = typed.trim();
        if typed.is_empty() {
            continue;
        }
        if typed.eq_ignore_ascii_case("q") {
            return Ok(());
        }
        if typed.eq_ignore_ascii_case("c") {
            match scan_descriptor(0) {
                Ok(ms) => break ms,
                Err(e) => {
                    eprintln!("\n{e}");
                    continue;
                }
            }
        }
        let text = if typed.contains("multi(") {
            typed.to_string()
        } else {
            let path = PathBuf::from(ui::expand_home(&clean_path(typed)));
            match fs::metadata(&path) {
                Ok(meta) if meta.is_file() && meta.len() <= 1_000_000 => match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("That file could not be read ({e}).");
                        continue;
                    }
                },
                Ok(_) => {
                    eprintln!("That is not a descriptor file.");
                    continue;
                }
                Err(_) => {
                    eprintln!("There is no file at \"{}\". Check it and try again.", path.display());
                    continue;
                }
            }
        };
        match MultisigWallet::from_descriptor_text(&text) {
            Ok(ms) => break ms,
            Err(e) => eprintln!("That cannot be registered: {e}"),
        }
    };
    if let Some((name, _)) = load_multisig_wallets().iter().find(|(_, w)| w.same_as(&ms)) {
        eprintln!("\nThis multisig wallet is already registered, as \"{name}\". Nothing was changed.");
        return Ok(());
    }
    let network = if ms.is_mainnet() { Network::Bitcoin } else { Network::Testnet };
    eprintln!("\nMultisig wallet: {} signatures needed, {}, {}", ms.label(), wallet_kind(&ms), net_words(ms.is_mainnet()));
    eprintln!("Cosigners:");
    for (i, c) in ms.cosigners.iter().enumerate() {
        eprintln!("  {}) {}  {:<16} {}", i + 1, c.fingerprint, path_to_display(&c.path), c.xpub);
    }
    let first = ms.address(0, 0, network)?;
    eprintln!("\nIts first receive address: {first}");
    eprintln!("Sparrow must show exactly this one (in the wallet's Addresses tab, the first receive address).");
    if !ui::confirm("\nDoes it match, and are these the right cosigners?", false)? {
        eprintln!("Nothing was registered.");
        return Ok(());
    }
    let dir = ensure_private_folder(backup_folder()?.join(MULTISIG_FOLDER))?;
    let default = format!("multisig-{}-of-{}", ms.threshold, ms.cosigners.len());
    let name = loop {
        let typed = ui::read_line(&format!("Name for this wallet [{default}]: "))?;
        let wanted = if typed.trim().is_empty() { default.clone() } else { typed.trim().to_string() };
        match clean_backup_name(&wanted) {
            Ok(name) if dir.join(format!("{name}.multisig")).exists() => {
                eprintln!("A multisig wallet named \"{name}\" is already registered. Choose another name.")
            }
            Ok(name) => break name,
            Err(e) => eprintln!("{e}"),
        }
    };
    let file = dir.join(format!("{name}.multisig"));
    let mut body = String::new();
    let _ = writeln!(body, "# SeedLock multisig wallet \"{name}\": {}, {}, {}", ms.label(), wallet_kind(&ms), net_words(ms.is_mainnet()));
    let cosigners: Vec<String> = ms.cosigners.iter().map(|c| format!("{} {}", c.fingerprint, path_to_display(&c.path))).collect();
    let _ = writeln!(body, "# Cosigners: {}", cosigners.join(", "));
    let _ = writeln!(body, "# First receive address: {first}");
    let _ = writeln!(body, "# No secrets in here, but anyone who reads it can see all the wallet's addresses.");
    let _ = writeln!(body, "{}", ms.descriptor()?);
    write_new_file(&file, body.as_bytes())?;
    eprintln!("\nRegistered as \"{name}\":\n  {}", full_path(&file).display());
    eprintln!("From now on SeedLock checks this wallet's transactions when it signs them (menu 6): every input, and");
    eprintln!("that change really returns to this wallet.");
    Ok(())
}

fn multisig_list() -> Res<()> {
    let wallets = load_multisig_wallets();
    if wallets.is_empty() {
        eprintln!("\nNo multisig wallet is registered yet (choose 2 in this menu to register one).");
        return Ok(());
    }
    for (name, ms) in &wallets {
        let network = if ms.is_mainnet() { Network::Bitcoin } else { Network::Testnet };
        eprintln!("\n\"{name}\": {}, {}, {}", ms.label(), wallet_kind(&ms), net_words(ms.is_mainnet()));
        for (i, c) in ms.cosigners.iter().enumerate() {
            eprintln!("  {}) {}  {}", i + 1, c.fingerprint, path_to_display(&c.path));
        }
        eprintln!("  first receive address: {}", ms.address(0, 0, network)?);
    }
    Ok(())
}

/// The address type the way Sparrow names it in its "Script Type" list.
fn sparrow_name(script: ScriptType) -> &'static str {
    match script {
        ScriptType::Wpkh => "Native Segwit (P2WPKH)",
        ScriptType::ShWpkh => "Nested Segwit (P2SH-P2WPKH)",
        ScriptType::Tr => "Taproot (P2TR)",
        ScriptType::Pkh => "Legacy (P2PKH)",
    }
}

/// The backup line (`SEEDLOCK1:...`) of a backup's text, without spaces and in capitals, to tell
/// whether two backups are the same one.
fn backup_line(text: &str) -> Option<String> {
    text.lines()
        .map(|l| l.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase())
        .find(|l| l.starts_with(backup::TEXT_PREFIX))
}

/// The name of a backup, for the files made from it and for Sparrow's keystore label: its file name.
/// For a backup read from a QR code (or pasted): when it is one of the backups in the backups folder,
/// that one's name; else the name its QR code carries (from SeedLock's phone app, or from version 0.2.0
/// of this program on); otherwise the person is asked (default "seedlock-backup").
fn named_backup(arg: &str) -> Res<String> {
    let arg = arg.trim();
    if !arg.to_uppercase().starts_with(backup::TEXT_PREFIX) {
        return Ok(backup_name(arg));
    }
    if let (Some(wanted), Ok(dir)) = (backup_line(arg), backup_folder()) {
        for (name, path) in list_backups_in(&dir) {
            let same = fs::read_to_string(&path).ok().and_then(|text| backup_line(&text)).is_some_and(|line| line == wanted);
            if same {
                eprintln!("\n(This QR code is the backup \"{name}\" in your backups folder, so its name is used.)");
                return Ok(name);
            }
        }
    }
    if let Some(name) = backup::name_in(arg).and_then(|carried| clean_backup_name(&carried).ok()) {
        eprintln!("\n(This QR code carries the backup's name, \"{name}\", so that name is used.)");
        return Ok(name);
    }
    if !ui::interactive() {
        return Ok(SCRIPT_DEFAULT_NAME.to_string());
    }
    eprintln!("\nThis QR code does not carry the backup's name, and this backup is not in your backups folder.");
    loop {
        let typed = ui::read_line(&format!(
            "What is this seed called? (It names the saved files and the keystore in Sparrow.) [{SCRIPT_DEFAULT_NAME}]: "
        ))?;
        if typed.trim().is_empty() {
            return Ok(SCRIPT_DEFAULT_NAME.to_string());
        }
        match clean_backup_name(&typed) {
            Ok(name) => return Ok(name),
            Err(e) => eprintln!("{e}"),
        }
    }
}

/// A short name for files made from a backup: its file name, or a default for a pasted or scanned backup.
fn backup_name(arg: &str) -> String {
    let arg = arg.trim();
    if arg.to_uppercase().starts_with(backup::TEXT_PREFIX) {
        return SCRIPT_DEFAULT_NAME.to_string();
    }
    find_backup_file(arg)
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| SCRIPT_DEFAULT_NAME.to_string())
}

/// The wallet laid out for Sparrow: its three fields (the derivation written the way Sparrow wants
/// it), its QR code (a UR crypto-output that also names Sparrow's keystore after the backup, `name`), a
/// one-line output descriptor, the other address types and the first addresses to compare. Shown,
/// offered as a QR code, and saved next to the backups.
fn wallet_for_sparrow(wallet: &Wallet, script: ScriptType, network: NetworkArg, passphrase_used: bool, found: &str, name: &str) -> Res<()> {
    use std::fmt::Write as _;
    let net = match network {
        NetworkArg::Mainnet => "mainnet (real bitcoin)",
        NetworkArg::Regtest => "regtest",
        _ => "a test network (testnet or signet)",
    };
    let body = script.wrap_descriptor(&format!("{}/<0;1>/*", wallet.xpub_with_origin(script, 0)?));
    let descriptor = format!("{body}#{}", seedlock_core::wallet::descriptor_checksum(&body)?);
    // The QR code is the same wallet as a UR crypto-output, which also names Sparrow's keystore after the backup.
    let label = multisig::keystore_label(name, wallet.fingerprint());
    let shortened = if !name.trim().is_empty() && label != name.trim() { " (Sparrow allows 16 characters)" } else { "" };
    let code = wallet.crypto_output_ur(script, 0, &label)?;
    let base = path_to_display(&wallet.account_path(script, 0)?);
    let mut t = String::new();
    let _ = writeln!(t, "Wallet {} for Sparrow: {}, {net}", wallet.fingerprint(), sparrow_name(script));
    let _ = writeln!(t, "({found})");
    if passphrase_used {
        let _ = writeln!(t, "With its BIP39 passphrase: a different wallet from the one without it.");
    }
    let _ = writeln!(t, "\nIn Sparrow: File > New Wallet, Script Type \"{}\", Keystore \"Watch Only Wallet\":", sparrow_name(script));
    let _ = writeln!(t, "  Master fingerprint : {}", wallet.fingerprint());
    let _ = writeln!(t, "  Derivation         : {base}");
    let _ = writeln!(t, "  xpub               : {}", wallet.account_xpub(script, 0)?);
    let _ = writeln!(t, "\nOr in one step: scan the wallet's QR code (File > Import Wallet, Output Descriptor, Scan...), which also");
    let _ = writeln!(t, "names its keystore \"{label}\"{shortened}; when Sparrow asks for the wallet's name, type \"{name}\" there too.");
    let _ = writeln!(t, "Or paste this output descriptor (File > New Wallet, then on the Settings tab, next to Descriptor, Edit...):");
    let _ = writeln!(t, "  {descriptor}");
    let _ = writeln!(t, "\nThe same wallet with the other address types (if you choose another Script Type in Sparrow):");
    for other in ScriptType::ALL.into_iter().filter(|s| s.purpose() != script.purpose()) {
        let path = path_to_display(&wallet.account_path(other, 0)?);
        let _ = writeln!(t, "  {:<28} {:<14} {}", sparrow_name(other), path, wallet.account_xpub(other, 0)?);
    }
    let _ = writeln!(t, "\nFirst receive addresses (Sparrow must show exactly these):");
    for i in 0..5 {
        let _ = writeln!(t, "  {:<20} {}", format!("{base}/0/{i}"), wallet.address(script, 0, false, i)?);
    }
    print!("\n{t}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    // saved before the QR code is offered, so that where they are is said whatever happens next
    let dir = ensure_backup_folder()?;
    let text_file = unused_path(&dir, &format!("{name}-for-sparrow"), "txt");
    write_new_file(&text_file, format!("{t}\nAs a QR code (UR crypto-output):\n  {code}\n").as_bytes())?;
    let picture = QrCode::with_error_correction_level(code.as_bytes(), EcLevel::M)?;
    let picture_file = unused_path(&dir, &format!("{name}-for-sparrow-qr"), "png");
    write_new_file(&picture_file, &qrimg::png(&picture, 8))?;
    eprintln!("\nSaved next to your backups. They hold no secrets, but anyone who reads them can see all your addresses:");
    eprintln!("  {}\n  {}", full_path(&text_file).display(), full_path(&picture_file).display());
    if ui::can_animate() && ui::confirm("\nShow the wallet as a QR code for Sparrow to scan?", true)? {
        show_wallet_qr(&code)?;
    }
    Ok(())
}

/// The wallet's QR code for Sparrow (a UR crypto-output): in a window of its own, else in the terminal.
fn show_wallet_qr(code: &str) -> Res<()> {
    show_text_qr(
        "SeedLock - wallet for Sparrow",
        code,
        "In Sparrow, choose File > Import Wallet, then Output Descriptor and Scan..., and hold the camera toward this code.",
    )
}

/// One QR code of `text`: in a window of its own, else drawn in the terminal.
fn show_text_qr(title: &str, text: &str, how_to_scan: &str) -> Res<()> {
    if qr_window_wanted() {
        match show_in_window(title, &[text.to_string()], EcLevel::M, how_to_scan) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(why) => eprintln!("\n(No QR window: {why}. The QR code is drawn in this terminal instead.)"),
        }
    }
    let art = ui::qr_terminal(text, EcLevel::M)?;
    apple_terminal_note();
    let (lines, columns) = ui::block_size(&art);
    if ui::wait_for_room(lines + 2, columns + 1, "QR code")? {
        eprintln!("\n{art}");
    }
    Ok(())
}

enum Source {
    File,
    Camera,
}

fn ask_transaction_source() -> Res<Source> {
    eprintln!("\nHow does the transaction get to this computer?");
    eprintln!("  1) From a file: the .psbt file that Sparrow saved (copied here, for example on a USB stick)");
    eprintln!("  2) With the camera: scan the QR code that Sparrow shows on the screen of your online computer");
    loop {
        match ui::read_line("Choose 1 or 2 [1]: ")?.trim() {
            "" | "1" => return Ok(Source::File),
            "2" => return Ok(Source::Camera),
            _ => eprintln!("Please type 1 or 2."),
        }
    }
}

/// The .psbt files in a folder (not the signed ones SeedLock wrote itself).
fn psbt_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("psbt")))
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            !name.contains(".signed") && !name.starts_with("signed-")
        })
        .collect();
    found.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    found
}

/// Help the person choose the PSBT file: list the .psbt files nearby, say what a PSBT is
/// and where it comes from, accept a typed or dragged-in path. `None` = go back to the menu.
fn pick_psbt_file() -> Res<Option<PathBuf>> {
    let here = std::env::current_dir().map_err(|e| format!("cannot find the current folder: {e}"))?;
    let folders = [here.clone(), transactions_folder()?];
    let found: Vec<PathBuf> = folders.iter().flat_map(|d| psbt_files_in(d)).collect();
    eprintln!("\nA \"PSBT\" is the unsigned transaction that Sparrow creates. As a file, its name ends in .psbt.");
    if found.is_empty() {
        eprintln!("\nNo .psbt files were found in these folders:");
        for d in &folders {
            eprintln!("  {}", d.display());
        }
        eprintln!("\nIn Sparrow, save the transaction to a file, then copy that file to this computer");
        eprintln!("(for example into the first folder above). Or type the full path of the file here:");
        eprintln!("tip: you can drag the file into this window to paste its path.");
    } else {
        eprintln!("\nPSBT files found:");
        for (i, p) in found.iter().enumerate() {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let folder = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            eprintln!("  {}) {name}    (in {folder})", i + 1);
        }
    }
    let prompt = if found.is_empty() {
        "Path of the .psbt file (or q to go back): ".to_string()
    } else {
        format!("Choose a file: number 1-{} (or type / drag in the path of another file, or q to go back): ", found.len())
    };
    loop {
        let typed = ui::expand_home(&clean_path(&ui::read_line(&format!("\n{prompt}"))?));
        if typed.is_empty() {
            continue;
        }
        if typed.eq_ignore_ascii_case("q") {
            return Ok(None);
        }
        if let Ok(n) = typed.parse::<usize>() {
            if (1..=found.len()).contains(&n) {
                return Ok(Some(found[n - 1].clone()));
            }
        }
        let path = PathBuf::from(&typed);
        if path.is_file() {
            return Ok(Some(path));
        }
        if path.is_dir() {
            eprintln!("That is a folder. Please type the path of the .psbt file itself.");
        } else {
            eprintln!("There is no file at \"{typed}\". Check the path and try again (or type q to go back).");
        }
    }
}

fn menu_sign() -> Res<()> {
    eprintln!("\n=== Sign a transaction ===");
    eprintln!("The payment is prepared in Sparrow (or another wallet) on your ONLINE computer. This computer checks it,");
    eprintln!("shows you every address and amount, and signs it only if you say yes. Your seed never leaves this computer.");
    let backup = pick_backup("Which backup holds the wallet that must sign?", true)?;
    let (tx, input) = match ask_transaction_source()? {
        Source::File => match pick_psbt_file()? {
            Some(path) => (read_psbt_file(&path)?, Some(path)),
            None => return Ok(()),
        },
        Source::Camera => (scan_transaction(0)?, None),
    };
    eprintln!("\nTransaction received: {} input(s), {} output(s).", tx.inputs.len(), tx.unsigned_tx.output.len());
    let passphrase = ui::confirm("Does this wallet use a BIP39 passphrase?", false)?;
    let args = SignArgs {
        backup,
        psbt: input.clone(),
        scan: false,
        camera: 0,
        out: None,
        network: None,
        passphrase,
        yes: false,
        qr: false,
        qr_terminal: false,
        qr_browser: false,
        static_qr: false,
    };
    sign_transaction(tx, input, &args)
}

fn menu_qr() -> Res<()> {
    qr_backup(QrArgs { backup: pick_backup("Which backup do you want to generate as a QR code?", false)?, svg: None }, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_paths_come_back_plain() {
        assert_eq!(clean_path("  wallet-one  "), "wallet-one");
        assert_eq!(clean_path("'/home/me/seedlocker backups/wallet one.seedlock' "), "/home/me/seedlocker backups/wallet one.seedlock");
        assert_eq!(clean_path("\"/home/me/w.seedlock\""), "/home/me/w.seedlock");
        assert_eq!(clean_path("file:///home/me/seedlocker%20backups/w%C3%A7.seedlock"), "/home/me/seedlocker backups/w\u{e7}.seedlock");
        assert_eq!(clean_path("file://localhost/tmp/a%zzb"), "/tmp/a%zzb");
        if cfg!(not(windows)) {
            // Apple's Terminal and iTerm2 put a backslash in front of spaces and other special characters
            assert_eq!(clean_path("/Users/me/seedlocker\\ backups/my\\ wallet\\ \\(old\\).seedlock "), "/Users/me/seedlocker backups/my wallet (old).seedlock");
        }
    }

    /// A transaction with one key path in its input (and optionally one in a change output).
    fn psbt_with_paths(input_path: Option<&str>, change_path: Option<&str>) -> Psbt {
        use seedlock_core::bitcoin::secp256k1::{Secp256k1, SecretKey};
        use seedlock_core::bitcoin::{absolute::LockTime, bip32::{DerivationPath, Fingerprint}, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn { previous_output: OutPoint::null(), script_sig: ScriptBuf::new(), sequence: Sequence::MAX, witness: Witness::new() }],
            output: vec![TxOut { value: Amount::from_sat(1000), script_pubkey: ScriptBuf::new() }],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        let key = SecretKey::from_slice(&[1u8; 32]).unwrap().public_key(&Secp256k1::new());
        let origin = |p: &str| (Fingerprint::from([1, 2, 3, 4]), p.parse::<DerivationPath>().unwrap());
        if let Some(p) = input_path {
            psbt.inputs[0].bip32_derivation.insert(key, origin(p));
        }
        if let Some(p) = change_path {
            psbt.outputs[0].bip32_derivation.insert(key, origin(p));
        }
        psbt
    }

    #[test]
    fn the_network_is_found_from_the_key_paths() {
        // (what each kind of key path says is tested in seedlock-core, psbt::network_hint)
        // --network always wins, and says exactly what was asked for
        let (network, label) = choose_network(&psbt_with_paths(Some("m/84'/1'/0'/0/3"), None), Some(NetworkArg::Signet), &[]).unwrap();
        assert!(matches!(network, NetworkArg::Signet) && label == "signet");
        let (network, label) = choose_network(&psbt_with_paths(Some("m/84'/1'/0'/0/3"), None), None, &[]).unwrap();
        assert!(matches!(network, NetworkArg::Testnet) && label.contains("testnet or signet"), "{label}");
        let (network, label) = choose_network(&psbt_with_paths(Some("m/84'/0'/0'/0/3"), None), None, &[]).unwrap();
        assert!(matches!(network, NetworkArg::Mainnet) && label.contains("real bitcoin"), "{label}");
        // nobody to ask (as in these tests): no key paths, or contradicting ones, give a clear error instead of a guess
        if !ui::interactive() {
            let none = choose_network(&psbt_with_paths(None, None), None, &[]).map(|_| ()).unwrap_err().to_string();
            assert!(none.contains("no key paths") && none.contains("--network"), "{none}");
            let both = choose_network(&psbt_with_paths(Some("m/84'/0'/0'/0/3"), Some("m/84'/1'/0'/1/0")), None, &[]).map(|_| ()).unwrap_err().to_string();
            assert!(both.contains("something is wrong"), "{both}");
        }
    }

    #[test]
    fn earlier_cosigner_files_are_recognised_for_their_folder() {
        for name in ["alice-cosigner.txt", "alice-cosigner-qr.png", "alice-cosigner-2.txt", "my wallet-cosigner-qr-3.png"] {
            assert!(is_cosigner_file(name), "{name}");
        }
        for name in ["alice.seedlock", "alice-for-sparrow.txt", "alice-cosigner.seedlock", "cosigner.txt", "alice-qr.png"] {
            assert!(!is_cosigner_file(name), "{name}");
        }
    }

    #[test]
    fn a_scanned_backup_is_matched_to_its_file_by_its_backup_line() {
        let file = "# SeedLock encrypted seed backup (format v1)\n# Argon2id + XChaCha20-Poly1305.\nSEEDLOCK1:KNGEWAIB AAAACAAD\n";
        assert_eq!(backup_line(file).as_deref(), Some("SEEDLOCK1:KNGEWAIBAAAACAAD"));
        assert_eq!(backup_line("seedlock1:kngewaibaaaacaad"), backup_line(file), "as a QR code gives it: one line, any case");
        assert_ne!(backup_line("SEEDLOCK1:KNGEWAIBAAAACAAE"), backup_line(file));
        assert_eq!(backup_line("# only comments\n"), None);
    }

    #[test]
    fn backup_names_are_cleaned_and_checked() {
        assert_eq!(clean_backup_name("  my wallet  ").unwrap(), "my wallet");
        assert_eq!(clean_backup_name("\"Savings 2026\"").unwrap(), "Savings 2026");
        assert_eq!(clean_backup_name("cold-storage.seedlock").unwrap(), "cold-storage");
        assert_eq!(clean_backup_name("cold-storage.SEEDLOCK").unwrap(), "cold-storage");
        assert_eq!(clean_backup_name("carteira Bitcoin \u{e7}\u{e3}o").unwrap(), "carteira Bitcoin \u{e7}\u{e3}o");
        for bad in ["", "   ", ".seedlock", "a/b", "..\\evil", "x:y", "what?", "a*b", ".hidden", "trailing.", "tab\there"] {
            assert!(clean_backup_name(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(clean_backup_name(&"x".repeat(101)).is_err());
        assert!(clean_backup_name(&"x".repeat(100)).is_ok());
    }

    #[test]
    fn taken_names_ignore_case_and_the_list_is_sorted() {
        let dir = std::env::temp_dir().join(format!("seedlock-unit-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for f in ["wallet-two.seedlock", "Alpha.seedlock", "wallet-one.seedlock", "notes.txt"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<String> = list_backups_in(&dir).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Alpha", "wallet-one", "wallet-two"]);
        assert_eq!(taken_name(&dir, "wallet-one").as_deref(), Some("wallet-one"));
        assert_eq!(taken_name(&dir, "Wallet-ONE").as_deref(), Some("wallet-one"));
        assert_eq!(taken_name(&dir, "alpha").as_deref(), Some("Alpha"));
        assert_eq!(taken_name(&dir, "wallet-three"), None);
        assert_eq!(taken_name(&dir, "notes"), None);
    }

    #[test]
    fn unused_paths_count_up_and_dragged_paths_are_cleaned() {
        let dir = std::env::temp_dir().join(format!("seedlock-unit-unused-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(unused_path(&dir, "w-qr", "png"), dir.join("w-qr.png"));
        fs::write(dir.join("w-qr.png"), b"x").unwrap();
        assert_eq!(unused_path(&dir, "w-qr", "png"), dir.join("w-qr-2.png"));
        assert_eq!(clean_path("  \"C:\\Users\\me\\tx.psbt\"  "), "C:\\Users\\me\\tx.psbt");
        assert_eq!(clean_path("'/home/me/my tx.psbt'"), "/home/me/my tx.psbt");
        if !cfg!(windows) {
            assert_eq!(clean_path("/home/me/my\\ tx.psbt"), "/home/me/my tx.psbt");
        }
    }

    #[test]
    fn only_unsigned_psbt_files_are_offered() {
        let dir = std::env::temp_dir().join(format!("seedlock-unit-psbt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for f in ["send.psbt", "Other.PSBT", "send.signed.psbt", "send.signed-2.psbt", "signed-1a2b3c4d.psbt", "notes.txt"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<String> = psbt_files_in(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Other.PSBT", "send.psbt"]);
    }

    #[test]
    fn signed_file_names_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("seedlock-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("tx.psbt");
        assert_eq!(default_signed_path(&input), dir.join("tx.signed.psbt"));
        fs::write(dir.join("tx.signed.psbt"), b"x").unwrap();
        assert_eq!(default_signed_path(&input), dir.join("tx.signed-2.psbt"));
        fs::write(dir.join("tx.signed-2.psbt"), b"x").unwrap();
        assert_eq!(default_signed_path(&input), dir.join("tx.signed-3.psbt"));
    }
}
