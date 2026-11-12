import Foundation
import Observation

/// A screen above the first one (the first one is `HomeScreen`).
enum Route: Hashable {
    case addBackup
    case backup(String)
    /// An opened backup: its fingerprint, and a comparison with the words on paper.
    case check(OpenSeed)
    /// An opened backup's words.
    case words(OpenSeed)
    case newSeed
    /// Dice rolls for a new seed of this many words.
    case dice(Int32)
    /// A new seed's words, to write on paper.
    case newWords(OpenSeed)
    /// Three of a new seed's words, typed back from the paper.
    case quiz(OpenSeed)
    /// The words of a seed one already has, typed in.
    case enterWords
    /// A seed encrypted into a new backup.
    case saveSeed(OpenSeed)
    case qr(QrShown)
    /// A backup's wallet laid out for Sparrow (the backup's name with it).
    case sparrow(OpenWallet, String)
    /// Signing a transaction with the backup of this name: where the transaction comes from.
    case sign(String)
    /// A transaction to check before signing it, with the wallet that signs it.
    case review(Signing)
    /// A signed transaction, to give back to Sparrow.
    case signed(SignedShown)
    /// Adding a multisig wallet made in Sparrow: where it comes from.
    case addMultisig
    /// A multisig wallet that was read, to check before it is added.
    case checkMultisig(MultisigShown)
    /// The multisig wallet kept on this iPhone under this name.
    case multisig(String)
    /// A backup's key for a multisig wallet, laid out for Sparrow.
    case cosigner(CosignerShown)
    case about

    /// The seed this screen holds, if any.
    var seed: OpenSeed? {
        switch self {
        case .check(let open), .words(let open), .newWords(let open), .quiz(let open), .saveSeed(let open):
            return open
        default:
            return nil
        }
    }

    /// What this screen holds and lets go of once no screen needs it any more: a seed, or a wallet.
    var held: Held? {
        switch self {
        case .sparrow(let open, _):
            return open
        case .review(let signing):
            return signing
        default:
            return seed
        }
    }

    /// True for the screens that may stay while SeedLock is left during the prompt for a backup's password: they hold
    /// nothing secret (a backup's screen, a multisig wallet's, and the start of signing, with the transaction to sign).
    var waitsWell: Bool {
        switch self {
        case .backup, .multisig, .sign:
            return true
        default:
            return false
        }
    }

    /// True for the screens with a secret on them: those that hold a seed, and the one where a seed's words are typed
    /// in. (The wallet for Sparrow shows no secret: an xpub, a descriptor and addresses.)
    var secret: Bool {
        seed != nil || self == .enterWords
    }

    var savesSeed: Bool {
        if case .saveSeed = self {
            return true
        }
        return false
    }
}

/// Something secret that a screen holds (a seed, or a wallet with its master key), until `close()` lets go of it: Swift
/// frees the object at once (by reference counting), and SeedLock's Rust code then wipes its memory.
protocol Held: AnyObject {
    func close()
}

/// A seed held while a screen needs it: one screen may hand it to the next (a new seed's words, then their check, then
/// its backup).
final class OpenSeed: Held, Hashable {
    private(set) var seed: Seed?

    init(_ seed: Seed) {
        self.seed = seed
    }

    func close() {
        seed = nil
    }

    static func == (a: OpenSeed, b: OpenSeed) -> Bool {
        a === b
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(ObjectIdentifier(self))
    }
}

/// A wallet held while its screen is open: a seed's wallet (with a BIP39 passphrase or without) for one network. It
/// holds the master key, and shows only what is public: its xpub and addresses.
final class OpenWallet: Held, Hashable {
    private(set) var wallet: Wallet?

    init(_ wallet: Wallet) {
        self.wallet = wallet
    }

    func close() {
        wallet = nil
    }

    static func == (a: OpenWallet, b: OpenWallet) -> Bool {
        a === b
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(ObjectIdentifier(self))
    }
}

/// A message on the screen.
struct AppMessage: Identifiable {
    let id = UUID()
    let title: String
    let text: String
}

/// What the password prompt opens a backup into.
enum Opening: Equatable {
    /// Its seed.
    case seed
    /// Its seed's wallet: the prompt also asks whether the wallet uses a BIP39 passphrase, and which; and, when
    /// the network is nil, whether it is for real bitcoin or for testing.
    case wallet(Net?)
}

/// A backup opened by the password prompt: its seed, or its seed's wallet.
enum Opened {
    case seed(Seed)
    case wallet(Wallet)
}

/// The prompt for a backup's password: which backup, what to know first (`note`), what to open it into, and what to do
/// with that once it is open.
struct PasswordRequest: Identifiable {
    let id = UUID()
    let name: String
    let text: String
    let memoryMib: Int32
    let note: String?
    let opening: Opening
    let then: (Opened) -> Void
}

/// A backup on this iPhone, with its wallet's fingerprint (without a BIP39 passphrase) once it is known here.
struct BackupEntry: Hashable {
    let name: String
    let fingerprint: String?

    /// The backup's name, with its fingerprint beside it when it is known: "Savings (73c5da0a)".
    var label: String {
        if let fingerprint {
            return "\(name) (\(fingerprint))"
        }
        return name
    }
}

/// What SeedLock shows and what it holds, on the screen's thread.
///
/// SeedLock works only while the iPhone is offline (`NetworkCheck`); online, everything closes at once and the first
/// screen only says why. Leaving SeedLock (another app, the home screen, the screen locked) closes every screen but the
/// first, and with them any open seed or wallet. Two exceptions, for at most 5 minutes each, are for a password manager
/// on this iPhone: while SeedLock asks for a backup's password, and no seed is open yet, the prompt stays while the
/// password is copied there (when the prompt closes, SeedLock empties the clipboard); and after a new seed's password
/// was copied for the password manager, the new seed's screen stays while it is pasted there (back in SeedLock, its
/// copy leaves the clipboard). A window of its own covers SeedLock's screen while it is left, while a secret is on the
/// screen and SeedLock is partly covered (the app switcher, Control Center), and while the screen is recorded or
/// mirrored (`PrivacyCover`). iOS lets no app refuse a screenshot: after one, SeedLock says what to do if a secret was
/// on the screen.
@Observable
final class AppModel {
    /// The screens above the first one, in order: the navigation stack's path.
    private(set) var path: [Route] = []
    /// Why the iPhone is online (empty while it is offline); nil until the first look.
    private(set) var online: [String]?
    /// The backups on this iPhone.
    private(set) var entries: [BackupEntry] = []
    /// A message on the screen, if any.
    var message: AppMessage?
    /// What SeedLock closed while it was left, said on the first screen until it was read.
    var notice: String?
    /// The prompt for a backup's password, while it is open.
    private(set) var prompt: PasswordRequest?
    /// True while a backup opens or a new one is saved (Argon2id takes a few seconds).
    private(set) var working = false

    let backups = Backups()
    let multisig = MultisigWallets()
    /// The names of the multisig wallets on this iPhone, for the first screen.
    private(set) var multisigNames: [String] = []
    private let network = NetworkCheck()
    private let cover = PrivacyCover()

    @ObservationIgnored private var started = false
    /// Counts the times everything was closed: work that ends afterwards (a backup that opens) shows nothing.
    @ObservationIgnored private var generation = 0
    /// False from the moment SeedLock is left until it is in front again.
    @ObservationIgnored private var inFront = true
    /// True while SeedLock is partly covered (the app switcher, Control Center, a call).
    @ObservationIgnored private var inactive = false
    /// When SeedLock was left while it asked for a backup's password and nothing else was open, else nil.
    @ObservationIgnored private var leftAtPrompt: ContinuousClock.Instant?
    /// True once SeedLock was left while the prompt now open was on the screen: the clipboard likely holds the
    /// password.
    @ObservationIgnored private var awayDuringPrompt = false
    /// Until when SeedLock may be left without closing what is open, after a new seed's password was copied for the
    /// password manager on this iPhone; nil otherwise.
    @ObservationIgnored private var keepOpenUntil: ContinuousClock.Instant?
    /// True when SeedLock was left within keepOpenUntil, and what was open stayed open.
    @ObservationIgnored private var keptOpen = false
    /// The clipboard's count of changes right after SeedLock put a secret there; nil when it put nothing there.
    @ObservationIgnored private var copiedCount: Int?
    /// True while the new seed's screen shows its generated password.
    @ObservationIgnored var passwordOnScreen = false

    /// How long SeedLock may be left for a password manager on this iPhone.
    static let keepOpenMinutes = 5

    /// Where SeedLock is: in front, partly covered, or left.
    enum Phase {
        case active, inactive, background
    }

    /// Starts watching the network and the screen (once, when the first screen appears).
    func start() {
        if started { return }
        started = true
        refreshBackups()
        refreshMultisig()
        network.start { [weak self] reasons in
            self?.networkChanged(reasons)
        }
        cover.recordingChanged = { [weak self] in
            self?.updateCover()
        }
        cover.screenshotTaken = { [weak self] in
            self?.screenshotTaken()
        }
        updateCover()
    }

    // MARK: Screens

    /// False while SeedLock is left, while the iPhone is online, and before the first look at the network: nothing
    /// new opens then.
    var canShow: Bool {
        inFront && online?.isEmpty == true
    }

    /// The navigation stack's path changes (Back, a swipe back, a link): the screens that went close, and a seed or a
    /// wallet goes once no screen holds it any more. Nothing new opens while SeedLock cannot show it.
    func setPath(_ wanted: [Route]) {
        let kept = canShow ? wanted : wanted.filter { path.contains($0) }
        let gone = (path + wanted).filter { !kept.contains($0) }
        path = kept
        let stillHeld = Set(kept.compactMap { route in route.held.map { ObjectIdentifier($0) } })
        for route in gone {
            if let open = route.held, !stillHeld.contains(ObjectIdentifier(open)) {
                open.close()
            }
        }
        if keepOpenUntil != nil && !kept.contains(where: { $0.savesSeed }) {
            endCopy()
        }
        if !kept.contains(where: { $0.savesSeed }) {
            passwordOnScreen = false
        }
    }

    /// Shows a new screen on top.
    func show(_ route: Route) {
        setPath(path + [route])
    }

    /// Shows a new screen in place of the top one, which closes (a seed it hands on stays).
    func showInstead(_ route: Route) {
        setPath(Array(path.dropLast()) + [route])
    }

    /// Back one screen.
    func back() {
        setPath(Array(path.dropLast()))
    }

    /// Back to the first screen: every other one closes, and lets go of what it holds.
    func home() {
        setPath([])
    }

    func say(_ title: String, _ text: String) {
        if canShow {
            message = AppMessage(title: title, text: text)
        }
    }

    /// True while a secret is on a screen (a seed, or words typed in).
    private var secretOpen: Bool {
        path.contains { $0.secret }
    }

    // MARK: Backups

    /// Reads the list of backups again (after one was added or deleted, or a fingerprint became known).
    func refreshBackups() {
        entries = backups.names().map { BackupEntry(name: $0, fingerprint: backups.fingerprint($0)) }
    }

    func entry(_ name: String) -> BackupEntry {
        entries.first { $0.name == name } ?? BackupEntry(name: name, fingerprint: nil)
    }

    func delete(_ name: String) {
        backups.delete(name)
        refreshBackups()
        home()
    }

    func refreshMultisig() {
        multisigNames = multisig.names()
    }

    /// The multisig wallet `name` was added: its screen shows, over the first one.
    func multisigAdded(_ name: String) {
        refreshMultisig()
        setPath([.multisig(name)])
        say("Multisig wallet added", "The multisig wallet \"\(name)\" is on this iPhone now. To sign one of its "
            + "transactions, choose \"Sign a transaction\" in a backup that is one of its cosigners (this screen "
            + "offers them, once they were opened on this iPhone): SeedLock checks every input of this wallet, and "
            + "that the change really returns to it.")
    }

    func deleteMultisig(_ name: String) {
        multisig.delete(name)
        refreshMultisig()
        home()
    }

    /// A new backup was saved, and checked (`saveNewBackup`): its screen opens, with what to do next. If SeedLock was
    /// left or went online meanwhile, the first screen says so instead.
    func newBackupSaved(_ name: String, fingerprint: String, started: Int) {
        backups.rememberFingerprint(name, fingerprint)
        refreshBackups()
        guard started == generation, canShow else {
            notice = "The backup \"\(name)\" was saved on this iPhone and checked while SeedLock was left: it opens "
                + "with its password."
            return
        }
        working = false
        setPath([.backup(name)])
        say(
            "Backup saved",
            "The encrypted backup \"\(name)\" is saved on this iPhone and checked: it opens with the password.\n\n"
                + "Next: keep a copy off this iPhone too (\"Save a copy as a file\", onto a USB stick or into a cloud "
                + "folder, or \"Show it as a QR code\"), and keep the password in your password manager, apart from "
                + "the backup."
        )
    }

    // MARK: The password prompt

    /// Asks for the password of the backup `name` (its text `text`), opens the backup and hands its seed to `then`.
    /// The prompt says `note` first, when there is one.
    func askPassword(
        name: String, text: String, memoryMib: Int32, note: String? = nil, then: @escaping (OpenSeed) -> Void
    ) {
        ask(PasswordRequest(name: name, text: text, memoryMib: memoryMib, note: note, opening: .seed) { opened in
            if case .seed(let seed) = opened {
                then(OpenSeed(seed))
            }
        })
    }

    /// Asks for the password of the backup `name` (its text `text`), and whether its wallet uses a BIP39 passphrase
    /// (and which); and for which network, when `network` is nil. The prompt says `note` first, when there is one.
    /// Opens the backup, and hands the seed's wallet to `then`.
    func askWallet(
        name: String, text: String, memoryMib: Int32, network: Net? = nil, note: String? = nil,
        then: @escaping (OpenWallet) -> Void
    ) {
        let opening = Opening.wallet(network)
        ask(PasswordRequest(name: name, text: text, memoryMib: memoryMib, note: note, opening: opening) { opened in
            if case .wallet(let wallet) = opened {
                then(OpenWallet(wallet))
            }
        })
    }

    private func ask(_ request: PasswordRequest) {
        guard canShow, prompt == nil else { return }
        awayDuringPrompt = false
        prompt = request
    }

    /// Closes the prompt. If SeedLock was left while it was open, the clipboard most likely holds the password
    /// (copied from a password manager): SeedLock empties it.
    func closePrompt() {
        guard prompt != nil else { return }
        prompt = nil
        working = false
        if awayDuringPrompt {
            awayDuringPrompt = false
            Clipboard.empty()
        }
    }

    /// Slow work starts (a backup that opens, a new one saved): from now on, leaving SeedLock closes everything. Gives
    /// the number to hand back when it ends.
    func workStarted() -> Int {
        working = true
        return generation
    }

    /// The backup of `request` opened, or not: what it opened into, and its wallet's fingerprint without a BIP39
    /// passphrase. Gives what the prompt says when it did not open; nil when it did, and when SeedLock was left or went
    /// online meanwhile: then what opened is let go of at once, and nothing shows.
    func unlocked(_ request: PasswordRequest, started: Int, result: Result<(Opened, String?), Error>) -> String? {
        guard started == generation, prompt?.id == request.id else { return nil }
        working = false
        switch result {
        case .success(let opened):
            if let fingerprint = opened.1 {
                backups.rememberFingerprint(request.name, fingerprint)
                refreshBackups()
            }
            closePrompt()
            request.then(opened.0)
            return nil
        case .failure(let error):
            if let e = error as? SeedLockError, case .WrongPassword = e {
                return "That password does not open this backup. Try again (upper and lower case count)."
            }
            return "Not opened: \(userText(error))"
        }
    }

    /// Slow work ended without a result to show (a new backup not saved).
    /// Work that began at `started` (`workStarted`) ended. True when what came of it may show: SeedLock was not left,
    /// nor went online, meanwhile (else everything closed, and what came of it is to be let go of).
    @discardableResult
    func workEnded(started: Int) -> Bool {
        guard started == generation else { return false }
        working = false
        return canShow
    }

    /// Several characters at once went into a password or a seed's words: pasted, most likely (or filled in by a
    /// password manager). The clipboard may hold them: SeedLock empties it, and gives what to say about that.
    func pastedSecret() -> String? {
        guard Clipboard.holdsText else { return nil }
        Clipboard.empty()
        awayDuringPrompt = false
        copiedCount = nil
        return "SeedLock emptied the clipboard, in case it held what went in."
    }

    // MARK: A new password for the password manager

    /// Puts a new seed's password on the clipboard, for the password manager on this iPhone: for this iPhone only
    /// (iOS does not hand it to a Mac or an iPad nearby), and iOS empties it after keepOpenMinutes. Meanwhile SeedLock
    /// may be left without closing what is open, to paste it there; the new seed's screen ends this when it closes
    /// (`endCopy`). Back in SeedLock, its copy leaves the clipboard, if it is still there (something copied since,
    /// such as the password copied back from the password manager, stays).
    func copySecret(_ text: String) {
        copiedCount = Clipboard.copySecret(text, minutes: AppModel.keepOpenMinutes)
        keepOpenUntil = ContinuousClock.now + .seconds(AppModel.keepOpenMinutes * 60)
    }

    /// The new seed's screen, which copied its password, closes: leaving SeedLock closes everything again, and
    /// SeedLock's copy leaves the clipboard.
    private func endCopy() {
        keepOpenUntil = nil
        emptyOwnCopy()
    }

    /// Empties the clipboard if it still holds what SeedLock put there.
    private func emptyOwnCopy() {
        if let count = copiedCount, Clipboard.stillHolds(count) {
            Clipboard.empty()
        }
        copiedCount = nil
    }

    // MARK: Leaving, and the network

    /// The scene's phase changed: SeedLock is in front, partly covered, or left.
    func phaseChanged(_ phase: Phase) {
        switch phase {
        case .active:
            inactive = false
            if !inFront {
                inFront = true
                returned()
            }
        case .inactive:
            inactive = true
        case .background:
            inactive = false
            if inFront {
                inFront = false
                left()
            }
        }
        updateCover()
    }

    /// True when all SeedLock shows is a prompt for a backup's password over the first screen, a backup's screen, and
    /// perhaps the start of signing: no seed or wallet is open then, nor anything else that leaving SeedLock must
    /// close.
    private var waitsForPassword: Bool {
        prompt != nil && !working && path.allSatisfy { $0.waitsWell }
    }

    private func left() {
        Brightness.qrShowing(false)
        if waitsForPassword {
            // Most likely gone to copy the password from a password manager on this iPhone. No backup is open while
            // SeedLock asks for its password, so the prompt and the screen below it stay; back after more than
            // keepOpenMinutes, they close (returned).
            leftAtPrompt = ContinuousClock.now
            awayDuringPrompt = true
            return
        }
        if !working, prompt == nil, let until = keepOpenUntil, ContinuousClock.now <= until {
            // gone to the password manager on this iPhone, to paste the new seed's password into it: back soon
            keptOpen = true
            return
        }
        closeEverything()
    }

    private func returned() {
        if let leftAt = leftAtPrompt {
            leftAtPrompt = nil
            if ContinuousClock.now - leftAt > .seconds(AppModel.keepOpenMinutes * 60) {
                closeEverything()
                notice = "You were away from SeedLock for longer than \(AppModel.keepOpenMinutes) minutes while it "
                    + "asked for a password, so it closed what was open, as it does whenever it is left."
            }
        }
        if keptOpen {
            keptOpen = false
            if let until = keepOpenUntil, ContinuousClock.now <= until {
                // back from the password manager: SeedLock's copy of the password leaves the clipboard
                emptyOwnCopy()
            } else {
                closeEverything()
                notice = "You were away from SeedLock for longer than \(AppModel.keepOpenMinutes) minutes while the "
                    + "password went to your password manager, so it closed what was open, as it does whenever it is "
                    + "left. A new seed's words are on your paper: \"Encrypt the words of a seed I already have\" "
                    + "makes its backup."
            }
        }
        // first of all: online, everything closes
        networkChanged(network.reasonsNow)
    }

    /// Every screen but the first closes, with the prompt and any message, and with them any open seed or wallet. Work
    /// under way (a backup that opens) shows nothing when it ends.
    func closeEverything() {
        generation += 1
        leftAtPrompt = nil
        keptOpen = false
        message = nil
        closePrompt()
        working = false
        setPath([])
        keepOpenUntil = nil
        emptyOwnCopy()
    }

    private func networkChanged(_ reasons: [String]) {
        online = reasons
        if !reasons.isEmpty {
            closeEverything()
        }
    }

    /// The cover is on while SeedLock is left, while it is partly covered with a secret on a screen, and while the
    /// screen is recorded or mirrored. Not while it is partly covered otherwise: iOS's AutoFill and file picker show
    /// while SeedLock is partly covered, and the cover would hide them.
    private func updateCover() {
        let recorded = cover.recorded
        if recorded || !inFront || (inactive && secretOpen) {
            cover.show(recorded: recorded)
        } else {
            cover.hide()
        }
    }

    /// A screenshot was taken: iOS lets no app refuse one, so SeedLock says what to do when a secret was on the screen.
    private func screenshotTaken() {
        guard let top = path.last else { return }
        let photos = "A copy of it is now in Photos, which iCloud may copy as soon as the iPhone is online."
        switch top {
        case .newWords:
            say("Screenshot", "A screenshot was taken while your new seed's words were on the screen. \(photos) Do not "
                + "use this seed: delete the screenshot in Photos (and in Recently Deleted), then create another seed.")
        case .words:
            say("Screenshot", "A screenshot was taken while the seed's words were on the screen. \(photos) Delete it "
                + "in Photos, and in Recently Deleted, before the iPhone goes online. If it was online since, move the "
                + "funds to a new seed.")
        case .saveSeed where passwordOnScreen:
            say("Screenshot", "A screenshot was taken while the new password was on the screen. \(photos) Delete it in "
                + "Photos, and in Recently Deleted, before the iPhone goes online.")
        case .check, .quiz, .enterWords, .saveSeed:
            say("Screenshot", "A screenshot was taken. If words or a password that you typed were on the screen: "
                + "\(photos) Delete it in Photos, and in Recently Deleted, before the iPhone goes online.")
        case .sparrow:
            say("Screenshot", "A screenshot was taken. It holds nothing that can spend, but whoever sees it can see "
                + "this wallet's addresses, and with the silent payment wallet every payment it receives. \(photos) "
                + "To keep it private, delete it in Photos, and in Recently Deleted, before the iPhone goes online.")
        default:
            break
        }
    }
}
