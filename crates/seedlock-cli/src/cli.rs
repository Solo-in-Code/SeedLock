//! Command-line definitions.

use clap::{Args, Parser, Subcommand, ValueEnum};
use seedlock_core::{backup::KdfParams, wallet::ScriptType, Network};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "seedlock",
    version,
    about = "Offline Bitcoin seed tool: password-encrypted mnemonic backups and PSBT signing, no special hardware needed.",
    after_help = "Run `seedlock` without a command for a simple step-by-step menu.\n\
                  This program never uses the network. Use it on an offline computer."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Create a new seed and save it as an encrypted backup
    New(NewArgs),
    /// Encrypt an existing mnemonic into a backup file
    Encrypt(EncryptArgs),
    /// Decrypt a backup and show the mnemonic words
    Decrypt(DecryptArgs),
    /// Check that a backup opens correctly (does not show the words)
    Verify(VerifyArgs),
    /// Show fingerprint, xpub, descriptors and addresses (for watch-only wallets)
    Info(InfoArgs),
    /// Review a PSBT and sign it
    Sign(SignArgs),
    /// Show the settings of a backup (no password needed)
    Inspect(InspectArgs),
    /// Show a backup as a QR code and save it as a picture in the backups folder
    Qr(QrArgs),
    /// Test the camera that is used to scan transactions from Sparrow
    Camera(CameraArgs),
    /// Generate a strong random password made of easy-to-type words
    Password(PasswordArgs),
    /// Find the valid last word of a mnemonic (dice / manual seed creation)
    FinalWord,
}

#[derive(Args, Clone)]
pub struct SaveOpts {
    /// Where to save the encrypted backup. Default: inside the "seedlocker backups" folder.
    /// An existing file is never overwritten.
    #[arg(short, long, value_name = "FILE")]
    pub out: Option<PathBuf>,
    /// Key-derivation strength: light (64 MiB), standard (256 MiB), strong (1 GiB)
    #[arg(long, value_enum, default_value_t = KdfPreset::Standard)]
    pub kdf: KdfPreset,
    /// Also show the encrypted backup as a QR code
    #[arg(long)]
    pub qr: bool,
}

#[derive(Args)]
pub struct NewArgs {
    /// Number of words: 12, 15, 18, 21 or 24
    #[arg(short, long, default_value_t = 24)]
    pub words: usize,
    /// Add your own dice rolls as extra randomness
    #[arg(long)]
    pub dice: bool,
    /// With --dice: use only the dice (same result as SeedSigner); no computer randomness is mixed in
    #[arg(long, requires = "dice")]
    pub dice_only: bool,
    /// Skip the "type three words" check after showing the seed
    #[arg(long)]
    pub no_quiz: bool,
    #[command(flatten)]
    pub save: SaveOpts,
}

#[derive(Args)]
pub struct EncryptArgs {
    #[command(flatten)]
    pub save: SaveOpts,
}

#[derive(Args)]
pub struct DecryptArgs {
    /// The name of a backup in the "seedlocker backups" folder, a .seedlock file, or the SEEDLOCK1:... text
    pub backup: String,
    /// Print only the words on stdout (for scripts)
    #[arg(long)]
    pub raw: bool,
}

#[derive(Args)]
pub struct VerifyArgs {
    /// The name of a backup in the "seedlocker backups" folder, a .seedlock file, or the SEEDLOCK1:... text
    pub backup: String,
    /// Fail (exit code 2) unless the wallet fingerprint equals this value
    #[arg(long, value_name = "HEX")]
    pub fingerprint: Option<String>,
    /// Also type your paper-backup words and check that they match this backup
    #[arg(long)]
    pub compare_words: bool,
}

#[derive(Args)]
pub struct InfoArgs {
    /// The name of a backup in the "seedlocker backups" folder, a .seedlock file, or the SEEDLOCK1:... text
    pub backup: String,
    #[arg(long, value_enum, default_value_t = NetworkArg::Mainnet)]
    pub network: NetworkArg,
    /// Script type / address format
    #[arg(long, value_enum, default_value_t = ScriptArg::Wpkh)]
    pub script: ScriptArg,
    /// Account number
    #[arg(long, default_value_t = 0)]
    pub account: u32,
    /// How many addresses to list
    #[arg(long, default_value_t = 5)]
    pub count: u32,
    /// Index of the first address to list
    #[arg(long, default_value_t = 0)]
    pub start: u32,
    /// List change addresses instead of receive addresses
    #[arg(long)]
    pub change: bool,
    /// Ask for a BIP39 passphrase (the "25th word")
    #[arg(long)]
    pub passphrase: bool,
}

#[derive(Args)]
pub struct SignArgs {
    /// The name of a backup in the "seedlocker backups" folder, a .seedlock file, or the SEEDLOCK1:... text
    pub backup: String,
    /// The PSBT to sign (binary, base64 or hex file)
    #[arg(long, value_name = "FILE", required_unless_present = "scan", conflicts_with = "scan")]
    pub psbt: Option<PathBuf>,
    /// Instead of a file: scan the transaction from the QR code that Sparrow shows, using this
    /// computer's camera (needs the free program ffmpeg)
    #[arg(long)]
    pub scan: bool,
    /// With --scan: which camera to use (0 is the first one; on Linux N means /dev/videoN)
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub camera: u32,
    /// Where to write the signed PSBT (default: next to the input file, or in the "seedlocker
    /// transactions" folder; an existing file is never overwritten)
    #[arg(short, long, value_name = "FILE")]
    pub out: Option<PathBuf>,
    /// The network (default: found from the key paths in the transaction; testnet, signet and
    /// regtest cannot be told apart that way, so a test network is shown with testnet addresses)
    #[arg(long, value_enum)]
    pub network: Option<NetworkArg>,
    /// Ask for a BIP39 passphrase (the "25th word")
    #[arg(long)]
    pub passphrase: bool,
    /// Do not ask "Sign this transaction?" (for scripts; the review is still printed)
    #[arg(long)]
    pub yes: bool,
    /// After signing, show the signed transaction as an animated QR code (the UR format SeedSigner
    /// uses) for Sparrow to scan: in a window of its own, like Sparrow's; if no window can be opened,
    /// in this terminal. Without a terminal, the frames are printed as text
    #[arg(long)]
    pub qr: bool,
    /// Like --qr, but drawn in this terminal instead of a window of its own
    #[arg(long, conflicts_with = "qr_browser")]
    pub qr_terminal: bool,
    /// Like --qr, but shown as a large page in your web browser (on Linux it needs xdg-open)
    #[arg(long, conflicts_with = "static_qr")]
    pub qr_browser: bool,
    /// One single static QR code instead of an animated one (only works for small transactions)
    #[arg(long, conflicts_with = "qr")]
    pub static_qr: bool,
}

#[derive(Args)]
pub struct InspectArgs {
    /// The name of a backup in the "seedlocker backups" folder, a .seedlock file, or the SEEDLOCK1:... text
    pub backup: String,
}

#[derive(Args)]
pub struct QrArgs {
    /// The name of a backup in the "seedlocker backups" folder, a .seedlock file, or the SEEDLOCK1:... text
    pub backup: String,
    /// Also write the QR code to an SVG file (for printing). A PNG picture is always saved in the backups folder.
    #[arg(long, value_name = "FILE")]
    pub svg: Option<PathBuf>,
}

#[derive(Args)]
pub struct CameraArgs {
    /// Which camera to test (0 is the first one; on Linux N means /dev/videoN)
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub camera: u32,
}

#[derive(Args)]
pub struct PasswordArgs {
    /// Number of words (each adds 11 bits of entropy)
    #[arg(short, long, default_value_t = 8)]
    pub words: usize,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum KdfPreset {
    Light,
    Standard,
    Strong,
}

impl KdfPreset {
    pub fn params(self) -> KdfParams {
        match self {
            KdfPreset::Light => KdfParams::LIGHT,
            KdfPreset::Standard => KdfParams::STANDARD,
            KdfPreset::Strong => KdfParams::STRONG,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum NetworkArg {
    Mainnet,
    Testnet,
    Signet,
    Regtest,
}

impl NetworkArg {
    pub fn network(self) -> Network {
        match self {
            NetworkArg::Mainnet => Network::Bitcoin,
            NetworkArg::Testnet => Network::Testnet,
            NetworkArg::Signet => Network::Signet,
            NetworkArg::Regtest => Network::Regtest,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ScriptArg {
    /// Native SegWit, bc1q... (BIP84). The usual choice.
    Wpkh,
    /// Taproot, bc1p... (BIP86)
    Tr,
    /// Nested SegWit, 3... (BIP49)
    ShWpkh,
    /// Legacy, 1... (BIP44)
    Pkh,
}

impl ScriptArg {
    pub fn script(self) -> ScriptType {
        match self {
            ScriptArg::Wpkh => ScriptType::Wpkh,
            ScriptArg::Tr => ScriptType::Tr,
            ScriptArg::ShWpkh => ScriptType::ShWpkh,
            ScriptArg::Pkh => ScriptType::Pkh,
        }
    }
}
