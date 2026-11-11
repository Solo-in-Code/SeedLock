// Calls SeedLock's library the way the iPhone app does, but on a computer: through the Swift code that
// uniffi-bindgen writes, with the app's own storage (Backups.swift), helpers (Helpers.swift), new backups
// (NewSeeds.swift), wallet for Sparrow as text (SparrowText.swift), camera pictures (CameraPicture.swift), texts for
// signing (SignText.swift) and reader of the license notices (Notices.swift).
// ios/swift-check/run.sh builds it and runs it; its one argument is the notices that `seedlock-tools notices --ios`
// wrote.
import Foundation

var failed = 0

func check(_ ok: Bool, _ what: String) {
    print(ok ? "OK      \(what)" : "FAILED  \(what)")
    if !ok {
        failed += 1
    }
}

/// The error that `work` throws, if any.
func failure(_ work: () throws -> Void) -> Error? {
    do {
        try work()
        return nil
    } catch {
        return error
    }
}

func isWrongPassword(_ error: Error?) -> Bool {
    if let e = error as? SeedLockError, case .WrongPassword = e {
        return true
    }
    return false
}

/// A QR code as the iPhone's camera gives it: drawn 4 to 6 pixels per module on a grey picture of 640 x 480 pixels,
/// each row followed by `padding` bytes that are not part of the picture (as the camera's memory may have them).
func cameraPicture(_ code: QrImage, padding: Int) -> (width: Int, height: Int, bytesPerRow: Int, bytes: [UInt8]) {
    let (width, height) = (640, 480)
    let n = Int(code.width)
    let px = min(max(height / (n + 8), 4), 6)
    let side = (n + 8) * px
    let (x0, y0) = ((width - side) / 2, (height - side) / 2)
    let bytesPerRow = width + padding
    // the padding is dark: kept in by mistake, it would spoil the picture
    var bytes = [UInt8](repeating: 0, count: bytesPerRow * height)
    for y in 0..<height {
        for x in 0..<width {
            bytes[y * bytesPerRow + x] = 170
        }
    }
    let modules = [UInt8](code.modules)
    for y in 0..<side {
        for x in 0..<side {
            let (mx, my) = (x / px - 4, y / px - 4)
            let dark = mx >= 0 && my >= 0 && mx < n && my < n && modules[my * n + mx] == 1
            bytes[(y0 + y) * bytesPerRow + x0 + x] = dark ? 35 : 230
        }
    }
    return (width, height, bytesPerRow, bytes)
}

/// What the scanner makes of a camera picture of `code`, given to it as the app gives it (`packedLuma`).
func read(_ scanner: QrScanner, _ code: QrImage, padding: Int) throws -> ScanStep {
    let picture = cameraPicture(code, padding: padding)
    let luma = picture.bytes.withUnsafeBytes { raw in
        packedLuma(raw.baseAddress!, width: picture.width, height: picture.height, bytesPerRow: picture.bytesPerRow)
    }
    return try scanner.addPicture(width: Int32(picture.width), height: Int32(picture.height), luma: luma)
}

let testWords = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
let password = Data("correct horse battery staple".utf8)

do {
    // the BIP39 test seed, its wallet's fingerprint, and the words given by their first four letters
    let seed = try Seed.fromWords(words: testWords)
    check(try seed.fingerprint() == "73c5da0a", "the BIP39 test seed's wallet fingerprint is 73c5da0a")
    let words = try seed.words()
    check(seed.wordCount() == 12 && words.joined(separator: " ") == testWords, "its 12 words, in order")
    check(try seed.matchesWords(words: "aban aban aban aban aban aban aban aban aban aban aban abou"),
          "the words on paper, by their first four letters, match")
    check(!(try seed.matchesWords(words: "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong")),
          "other words do not match")
    let badWord = failure {
        _ = try Seed.fromWords(words: testWords.replacingOccurrences(of: "about", with: "abandonx"))
    }
    check(badWord.map { userText($0).contains("abandonx") } == true,
          "a wrong word is refused, saying which: \(badWord.map(userText) ?? "no error")")

    // a backup made and opened again; a wrong password gives SeedLockError.WrongPassword, the error the prompt asks
    // again for (the light setting, to be quick)
    let backup = try seed.encrypt(password: password, strength: .light)
    let info = try backupInfo(text: backup.fileContents)
    check(info.memoryMib == 64 && info.version >= 1, "the backup says what opening it takes: \(info.memoryMib) MiB")
    check(backup.fileContents.hasSuffix(backup.text + "\n") || backup.fileContents.contains(backup.text),
          "the backup's file holds its backup line")
    let opened = try Seed.unlock(backupText: backup.fileContents, password: password)
    check(opened.sameAs(other: seed), "opened with its password, it is the same seed")
    let wrong = failure { _ = try Seed.unlock(backupText: backup.text, password: Data("not the password".utf8)) }
    check(isWrongPassword(wrong),
          "a wrong password gives SeedLockError.WrongPassword: \(wrong.map(userText) ?? "no error")")
    let damaged = failure { _ = try backupInfo(text: "SEEDLOCK1:not-a-backup") }
    check(damaged != nil && !isWrongPassword(damaged),
          "a damaged backup is refused: \(damaged.map(userText) ?? "no error")")

    // names, and the backup's QR code that carries its name
    check(try checkBackupName(typed: " \"Savings.seedlock\" ") == "Savings",
          "a name is cleaned up as the computer does")
    let badName = failure { _ = try checkBackupName(typed: "a/b") }
    check(badName.map { userText($0).contains("'/'") } == true,
          "a name with / is refused: \(badName.map(userText) ?? "no error")")
    let qr = try backupQrText(text: backup.fileContents, name: "Savings")
    check(backupNameIn(text: qr) == "Savings" && backupNameIn(text: backup.text) == nil,
          "the QR code's text carries the name")
    check(try backupText(text: qr) == backup.text, "and is the same backup")

    // the app's storage (Backups.swift), in a folder of its own
    let folder = FileManager.default.temporaryDirectory.appending(path: "seedlock-swift-check-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: folder) }
    let backups = Backups(dir: folder)
    check(backups.names().isEmpty, "no backups at first")
    try backups.save("Savings", contents: try backupFileContents(text: qr))
    try backups.save("another", contents: backup.fileContents)
    check(backups.names() == ["another", "Savings"], "two backups, in alphabetical order: \(backups.names())")
    check(backups.taken("SAVINGS") == "Savings", "a name is taken whatever its upper and lower case")
    let twice = failure { try backups.save("savings", contents: backup.fileContents) }
    check(twice.map { userText($0) == "There is already a backup named \"Savings\" on this iPhone." } == true,
          "a backup is never replaced: \(twice.map(userText) ?? "no error")")
    let other = try Seed.generate(words: 12).encrypt(password: password, strength: .light)
    check(backups.same(backup.text) != nil && backups.same(other.text) == nil,
          "the same backup is found by its backup line, another one is not")
    let saved = try backups.read("Savings")
    let savedLine = try backupText(text: saved)
    check(saved.hasPrefix("#") && savedLine == backup.text,
          "its file is the computer's: comment lines, then the backup line")
    check(try Seed.unlock(backupText: saved, password: password).sameAs(other: seed),
          "the saved backup opens with its password")
    check(backups.fingerprint("Savings") == nil, "no fingerprint before it was opened")
    backups.rememberFingerprint("Savings", "73c5da0a")
    backups.rememberFingerprint("another", "not hex!")
    backups.rememberFingerprint("missing", "73c5da0a")
    check(backups.fingerprint("Savings") == "73c5da0a" && backups.fingerprint("another") == nil
          && backups.fingerprint("missing") == nil,
          "a fingerprint is kept only for a backup there, and only a real one")
    check(backups.names(withFingerprintIn: ["0f056943", "73c5da0a"]) == ["Savings"]
            && backups.names(withFingerprintIn: ["0f056943"]).isEmpty,
          "the backups that are a transaction's wallets, by their fingerprints")
    backups.delete("Savings")
    check(backups.names() == ["another"] && backups.fingerprint("Savings") == nil, "deleting takes the fingerprint too")

    // a file that is too large is not read into memory
    let large = folder.appending(path: "large.seedlock")
    try Data(count: 70_000).write(to: large)
    let tooLarge = failure { _ = try readFile(large, limit: 64 * 1024, what: "a SeedLock backup") }
    check(tooLarge.map(userText) == "That file is too large to be a SeedLock backup.", "a file too large is refused")

    // a new seed (Create a new seed and its backup): its words, with dice rolls or without
    let fresh = try Seed.generate(words: 24)
    let freshWords = try fresh.words()
    check(fresh.wordCount() == 24 && freshWords.count == 24, "a new seed of 24 words")
    check(!(try Seed.generate(words: 12)).sameAs(other: try Seed.generate(words: 12)), "two new seeds differ")
    let oddCount = failure { _ = try Seed.generate(words: 13) }
    check(oddCount != nil, "13 words are refused: \(oddCount.map(userText) ?? "no error")")
    let need = try diceRollsNeeded(words: 12, diceOnly: true)
    let rolls = String(repeating: "123456", count: Int(need) / 6 + 1)
    check(try Seed.fromDice(rolls: rolls, words: 12, diceOnly: true)
            .sameAs(other: try Seed.fromDice(rolls: rolls, words: 12, diceOnly: true)),
          "the same dice rolls alone give the same seed (\(need) rolls for 12 words)")
    check(!(try Seed.fromDice(rolls: rolls, words: 12, diceOnly: false))
            .sameAs(other: try Seed.fromDice(rolls: rolls, words: 12, diceOnly: false)),
          "mixed with the system's randomness, the same rolls give different seeds")
    check(try countDiceRolls(text: "1 2 3, 4") == 4, "dice rolls are counted, spaces and commas left out")
    let badRoll = failure { _ = try countDiceRolls(text: "127") }
    check(badRoll.map { userText($0).contains("7") } == true,
          "a digit that is no roll is refused, saying which: \(badRoll.map(userText) ?? "no error")")

    // the quick check of the words written on paper (QuizScreen), and how they are shown (WordList)
    let picks = quizPicks(count: 24)
    check(picks.count == 3 && Set(picks).count == 3 && picks == picks.sorted()
            && picks.allSatisfy { (0..<24).contains($0) },
          "the quick check asks for three different words, in order: \(picks.map { $0 + 1 })")
    check(sameWord(" Abandon ", "abandon") && sameWord("aban", "abandon") && sameWord("zoo", "zoo"),
          "a word typed back counts whole, or by its first letters")
    check(sameWord("aba", "abandon") && !sameWord("abs", "absent") && !sameWord("ab", "abandon")
            && !sameWord("abandonx", "abandon") && !sameWord("about", "abandon"),
          "but not by fewer than 3 letters, or letters that other words start with too (abs: absent, absorb...)")
    check(numberedWord(0, "abandon") == " 1. abandon" && numberedWord(23, "zoo") == "24. zoo",
          "words are numbered as on paper")

    // a new backup saved and checked (SaveSeedScreen), with a password that SeedLock makes
    let made = try generatePassword()
    check(made.split(separator: "-").count == 8 && made.count >= Int(minPasswordLength()),
          "a password made by SeedLock: 8 words joined by dashes")
    let madeBytes = Data(made.utf8)
    let fingerprint = try saveNewBackup(fresh, name: "New wallet", password: madeBytes, strength: .light, in: backups)
    check(fingerprint == (try fresh.fingerprint()) && backups.names().contains("New wallet"),
          "a new backup is saved, and gives its wallet's fingerprint")
    check(try Seed.unlock(backupText: try backups.read("New wallet"), password: madeBytes).sameAs(other: fresh),
          "the new backup opens with its password, to the same seed")
    let sameName = failure {
        _ = try saveNewBackup(fresh, name: "NEW WALLET", password: madeBytes, strength: .light, in: backups)
    }
    check(sameName.map { userText($0).contains("already a backup named \"New wallet\"") } == true,
          "a new backup never replaces one: \(sameName.map(userText) ?? "no error")")
    let tooShort = failure {
        _ = try saveNewBackup(fresh, name: "Short", password: Data("short".utf8), strength: .light, in: backups)
    }
    check(tooShort != nil && !backups.names().contains("Short"),
          "a password too short is refused, and nothing is saved: \(tooShort.map(userText) ?? "no error")")

    // the QR code of a backup, as the app draws it (QrScreen): one value per module, dark or light
    let code = try qrCode(text: qr)
    let width = Int(code.width)
    let modules = [UInt8](code.modules)
    check(modules.count == width * width && modules.allSatisfy { $0 <= 1 }
            && modules[0] == 1 && modules[width + 1] == 0,
          "a backup's QR code: \(width) by \(width) modules, its corner's finder pattern in place")

    // the wallet for Sparrow (SparrowScreen), opened as the password prompt opens it: the backup with its password,
    // then the seed's wallet for the network chosen, with a BIP39 passphrase or without
    let unlocked = try Seed.unlock(backupText: backup.text, password: password)
    let wallet = try unlocked.wallet(passphrase: Data(), network: .mainnet)
    check(wallet.fingerprint() == "73c5da0a" && wallet.network() == .mainnet,
          "the backup's wallet for real bitcoin, without a passphrase: fingerprint 73c5da0a")
    let sparrow = try wallet.sparrow(addressType: .nativeSegwit, name: "my wallet")
    let bip84 = "xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3XyuvPEbvqAQY3rAPshWcMLoP2fMFMKHPJ4ZeZXYV"
        + "UhLv1VMrjPC7PW6V"
    check(sparrow.xpub == bip84 && sparrow.derivation == "m/84'/0'/0'" && sparrow.firstAddresses.count == 5
            && sparrow.firstAddresses[0].address == "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu",
          "its xpub, derivation and first of 5 addresses are BIP84's test vector")
    check(sparrow.qr.hasPrefix("UR:CRYPTO-OUTPUT/") && sparrow.label == "my wallet"
            && sparrow.descriptor.hasPrefix("wpkh([73c5da0a/84h/0h/0h]xpub6CatWdiZ"),
          "its QR code for Sparrow (a UR crypto-output that names the keystore), and its descriptor")
    let named = addressTypes.map { kind in (try? wallet.sparrow(addressType: kind, name: "x"))?.addressType ?? "" }
    check(named == addressTypes.map(sparrowName) && sparrow.otherTypes.map(\.name) == Array(named.dropFirst()),
          "the app names the 4 address types as Sparrow does, in the same order: \(named)")
    let file = sparrowFile(sparrow, name: "my wallet")
    let fileLines = file.components(separatedBy: "\n")
    check(fileLines.first == "Wallet 73c5da0a for Sparrow: Native Segwit (P2WPKH), mainnet (real bitcoin)"
            && fileLines.contains("  xpub               : \(sparrow.xpub)")
            && fileLines.contains { line in
                line.hasPrefix("  m/84'/0'/0'/0/4 ") && line.hasSuffix(" " + sparrow.firstAddresses[4].address)
            }
            && file.hasSuffix("As a QR code (UR crypto-output):\n  \(sparrow.qr)\n") && !file.contains("passphrase"),
          "the wallet as a text file, as the computer saves it")
    let long = try wallet.sparrow(addressType: .taproot, name: "Savings of the family")
    let longFile = sparrowFile(long, name: "Savings of the family")
    check(long.label == "Savings of the f" && longFile.contains("keystore \"Savings of the f\" (Sparrow allows 16 "
            + "characters); when Sparrow asks for the wallet's name, type \"Savings of the family\" there too.")
            && longFile.contains("Taproot (P2TR)") && longFile.contains("(the address type chosen in SeedLock;"),
          "a long name: Sparrow's keystore gets its first 16 characters, and the file says so")
    let withPassphrase = try unlocked.wallet(passphrase: Data("TREZOR".utf8), network: .testnet)
    let test = try withPassphrase.sparrow(addressType: .taproot, name: "Test")
    check(withPassphrase.fingerprint() != "73c5da0a" && test.passphraseUsed && test.xpub.hasPrefix("tpub")
            && test.firstAddresses.allSatisfy { $0.address.hasPrefix("tb1p") } && test.derivation == "m/86'/1'/0'"
            && test.network == "a test network (testnet or signet)"
            && sparrowFile(test, name: "Test").contains("\nWith its BIP39 passphrase: a different wallet"),
          "with a BIP39 passphrase and for testing: another wallet (\(withPassphrase.fingerprint())), tb1p addresses")
    let silent = try wallet.silentPayments(name: "my wallet")
    let silentFile = silentPaymentFile(silent, name: "my wallet")
    check(silent.address.hasPrefix("sp1q") && silent.descriptor.hasPrefix("sp([73c5da0a/352h/0h/0h]spscan1q")
            && silent.spscanName == "spscan" && silent.derivation == "m/352'/0'/0'" && !silent.passphraseUsed
            && silentFile.contains("\n  \(silent.address)\n")
            && silentFile.contains("\n  spscan             : \(silent.spscan)\n")
            && silentFile.contains("Policy Type \"\(sparrowSilentPolicy)\""),
          "the silent payment wallet, and its text file")
    let testSilent = try unlocked.wallet(passphrase: Data(), network: .testnet).silentPayments(name: "my wallet")
    let testSilentFile = silentPaymentFile(testSilent, name: "my wallet")
    check(testSilent.address.hasPrefix("tsp1q") && testSilent.spscanName == "tspscan"
            && testSilentFile.contains("\n  tspscan            : \(testSilent.spscan)\n"),
          "for testing: a tsp1q address, and the field tspscan")
    let walletCode = try qrCode(text: sparrow.qr)
    let silentCode = try qrCode(text: silent.descriptor)
    check(walletCode.width >= 21 && silentCode.width >= 21,
          "each fits one QR code: \(walletCode.width) and \(silentCode.width) modules wide")
    check(sparrowFileName("my wallet", silent: false, picture: false) == "my wallet-for-sparrow"
            && sparrowFileName("my wallet", silent: true, picture: true) == "my wallet-silent-payments-for-sparrow-qr",
          "the files are named as the computer names them")

    // signing (SignScreen, ReviewScreen, SignedScreen), with transactions that embit (a Python library) made for the
    // BIP39 test seed: 100 000 sat in from m/84'/0'/0'/0/3, 60 000 out to BIP 173's example address, 39 000 change
    // to m/84'/0'/0'/1/7; the same on a test network (m/84'/1'/0'/...); and the first without the key paths that
    // say whose it is
    let mainnetPsbt =
        "cHNidP8BAHECAAAAAaqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqAQAAAAD/////AmDqAAAAAAAAFgAUdR526BmRltRU"
        + "lBxF0bOjI/FDO9ZYmAAAAAAAABYAFIYu11IIaQndzcgNyUJ2ba2bnd+yAAAAAAABAR+ghgEAAAAAABYAFEfoz6Hib/xGBGg0ipyy"
        + "GNA0ajd4IgYD3nSQvMqSovtX14LD/WBUjOOoQsrW86jU520fL/f824kYc8XaClQAAIAAAACAAAAAgAAAAAADAAAAAAAiAgPrYVVh"
        + "1jKyG54krwvwKdkt81CmEZMsofn7LBFfa/nlExhzxdoKVAAAgAAAAIAAAACAAQAAAAcAAAAA"
    let testnetPsbt =
        "cHNidP8BAHECAAAAAaqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqAQAAAAD/////AmDqAAAAAAAAFgAUdR526BmRltRU"
        + "lBxF0bOjI/FDO9ZYmAAAAAAAABYAFDHUORI3eUFKBYByM9r5KsMZw49aAAAAAAABAR+ghgEAAAAAABYAFCTCiGndDq5eMJ6TzfzD"
        + "LlOOoE3xIgYCsVcfcMDVK6jUka+g6Ykfg8vFWMdNElCTB+3RiKv4APQYc8XaClQAAIABAACAAAAAgAAAAAADAAAAAAAiAgK2fM1D"
        + "8UQqUfrMtf3KRCSFrgEbAdK5z/KYzrIVmCUB+xhzxdoKVAAAgAEAAIAAAACAAQAAAAcAAAAA"
    let barePsbt =
        "cHNidP8BAHECAAAAAaqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqAQAAAAD/////AmDqAAAAAAAAFgAUdR526BmRltRU"
        + "lBxF0bOjI/FDO9ZYmAAAAAAAABYAFIYu11IIaQndzcgNyUJ2ba2bnd+yAAAAAAABAR+ghgEAAAAAABYAFEfoz6Hib/xGBGg0ipyy"
        + "GNA0ajd4AAAA"
    let tx = try Transaction.fromBytes(data: Data(base64Encoded: mainnetPsbt) ?? Data())
    check(tx.networkHint() == .mainnet && tx.networkHintWith(multisig: []) == .mainnet
            && tx.inputFingerprints() == ["73c5da0a"] && tx.inputCount() == 1 && tx.outputCount() == 2,
          "a transaction from its .psbt file: mainnet, for the wallet 73c5da0a, 1 input and 2 outputs")
    let fromText = try Transaction.fromBytes(data: Data(mainnetPsbt.utf8))
    check(fromText.toBytes() == tx.toBytes() && tx.toBase64() == mainnetPsbt, "the same from its base64 text")
    let review = try wallet.review(transaction: tx, multisig: [])
    check(review.fingerprint == "73c5da0a" && review.inputs.count == 1 && review.inputs[0].mine
            && review.inputs[0].owner == "yours (m/84'/0'/0'/0/3)",
          "review (\(review.network)): the input is the wallet's own: \(review.inputs.first?.owner ?? "none")")
    check(review.outputs.count == 2 && review.outputs[0].external && review.outputs[0].amount == "0.00060000"
            && review.outputs[0].address == "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4" && !review.outputs[1].external
            && review.outputs[1].note.hasPrefix("change back to you (verified from your seed: m/84'/0'/0'/1/7)"),
          "review: output 0 goes to someone else, output 1 is change back to the wallet")
    check(review.totalIn == "0.00100000" && review.fee == "0.00001000" && review.sending == "0.00060000"
            && review.inputsToSign == 1 && review.multisig == nil,
          "review: 0.00100000 in, a fee of 0.00001000, 0.00060000 leaving the wallet, 1 input to sign; warnings: "
            + "\(review.warnings)")
    switch Result(catching: { try withPassphrase.review(transaction: tx, multisig: []) }) {
    case .success(let other):
        check(other.inputsToSign == 0, "another wallet (a passphrase, a test network) has nothing to sign in it")
    case .failure(let error):
        check(true, "another wallet (a passphrase, a test network) cannot sign it: \(userText(error))")
    }
    let signed = try wallet.sign(transaction: tx, multisig: [])
    let signedBytes = signed.transaction.toBytes()
    check(signed.inputs == 1 && signed.multisig == nil && signedBytes.count > tx.toBytes().count
            && tx.toBytes() == fromText.toBytes(),
          "signed 1 input (\(signedBytes.count) bytes); the transaction given stays as it was")
    check(signedFileName(txid: signed.transaction.txid()) == "signed-\(tx.txid().prefix(8)).psbt"
            && inputsText(1) == "1 input" && inputsText(3) == "3 inputs",
          "the signed transaction's file is named after its id: \(signedFileName(txid: signed.transaction.txid()))")

    // the signed transaction as an animated QR code (SignedScreen), read back by the camera's reader, given the
    // pictures as the app gives them (CameraReader: their rows packed by packedLuma)
    let tiny: [UInt8] = [1, 2, 3, 0, 0, 4, 5, 6, 0, 0]
    let packed = tiny.withUnsafeBytes { packedLuma($0.baseAddress!, width: 3, height: 2, bytesPerRow: 5) }
    check(packed == Data([1, 2, 3, 4, 5, 6]), "a camera picture's rows, without their padding")
    let frames = try signed.transaction.qrFrames()
    let codes = try qrAnimation(frames: frames)
    check(frames.count >= 2 && frames.allSatisfy { $0.hasPrefix("UR:CRYPTO-PSBT/") } && codes.count == frames.count
            && codes.allSatisfy { $0.width == codes[0].width },
          "\(codes.count) animated QR codes, all \(codes.first?.width ?? 0) modules wide")
    let scanner = QrScanner(target: .transaction)
    var done = false
    var counted = false
    for i in 0..<(codes.count * 3) where !done {
        let step = try read(scanner, codes[(i + 1) % codes.count], padding: 64)
        counted = counted || (step.total > 0 && step.message?.contains("parts received") == true)
        done = step.done
    }
    check(done && counted && scanner.transaction()?.toBytes() == signedBytes,
          "read back by the camera's reader, part after part: the same signed transaction")
    let backupCode = try qrCode(text: try backupQrText(text: backup.fileContents, name: "Poupança"))
    let backupScanner = QrScanner(target: .backup)
    let backupStep = try read(backupScanner, backupCode, padding: 0)
    let scannedBackup = backupScanner.backupText() ?? ""
    check(backupStep.done && backupNameIn(text: scannedBackup) == "Poupança"
            && (try? backupText(text: scannedBackup)) == backup.text,
          "a backup's QR code read by the camera's reader, with its name")
    let testTx = try Transaction.fromBytes(data: Data(base64Encoded: testnetPsbt) ?? Data())
    let bare = try Transaction.fromBytes(data: Data(base64Encoded: barePsbt) ?? Data())
    check(testTx.networkHint() == .testNetwork && bare.networkHint() == .unknown && bare.inputFingerprints().isEmpty,
          "a test network's transaction says so; one without key paths says nothing (the prompt then asks)")
    let forTesting = try unlocked.wallet(passphrase: Data(), network: .testnet)
    let testReview = try forTesting.review(transaction: testTx, multisig: [])
    check(testReview.inputsToSign == 1 && testReview.outputs[0].address.hasPrefix("tb1q")
            && testReview.outputs[1].note.contains("m/84'/1'/0'/1/7"),
          "the test network's transaction, checked by the wallet for testing (\(testReview.network))")
    let notTransaction = failure { _ = try Transaction.fromBytes(data: Data("not a transaction".utf8)) }
    check(notTransaction != nil, "a file that is not a transaction is refused: \(notTransaction.map(userText) ?? "")")
    check(otherBackupText(signers: ["0f056943"], ours: "73c5da0a", here: ["Savings"])
            == "This transaction is for the wallet 0f056943. This backup is the wallet 73c5da0a, so it cannot sign it, "
            + "unless you use it with a BIP39 passphrase (then it is another wallet).\n\nThe backup for it on this "
            + "iPhone: \"Savings\"."
            && otherBackupText(signers: ["0f056943", "12345678"], ours: "73c5da0a", here: [])
            .hasPrefix("This transaction is for the wallets 0f056943, 12345678. This backup")
            && !otherBackupText(signers: ["0f056943"], ours: "73c5da0a", here: []).contains("on this iPhone"),
          "another backup's transaction: what SeedLock says")

    // slow work runs on a thread of its own, and its result comes back on the main thread, as in the app
    var result: Result<(Seed, String?), Error>?
    var workedAway = false
    var cameBack = false
    inBackground { () throws -> (Seed, String?) in
        workedAway = !Thread.isMainThread
        let seed = try Seed.unlock(backupText: backup.text, password: password)
        return (seed, try? seed.fingerprint())
    } done: { done in
        cameBack = Thread.isMainThread
        result = done
    }
    let deadline = Date().addingTimeInterval(120)
    while result == nil && Date() < deadline {
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
    }
    if case .success(let opened)? = result {
        check(opened.0.sameAs(other: seed) && opened.1 == "73c5da0a" && workedAway && cameBack,
              "a backup opened away from the main thread, its result back on it")
    } else {
        check(false, "a backup opened away from the main thread: \(String(describing: result))")
    }
} catch {
    check(false, "unexpected error: \(userText(error))")
}

// the license notices, as the About screen reads them
if CommandLine.arguments.count > 1, let text = try? String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8) {
    let notices = Notices(text: text)
    let headings = notices.parts.map(\.heading)
    check(text.hasPrefix("SeedLock \(seedlockVersion()) for iPhone: licenses"),
          "the notices are this version's, for iPhone")
    check(headings.first == "" && (notices.parts.first?.paragraphs.first ?? "").hasPrefix("SeedLock is free software"),
          "the notices start with what they are")
    for wanted in ["SeedLock", "The Rust libraries in SeedLock's library (libseedlock_ffi.a)", "The app's Swift part",
                   "The MIT License", "Apache License, Version 2.0", "Mozilla Public License, Version 2.0"] {
        check(headings.contains(wanted), "the notices' heading \"\(wanted)\"")
    }
    let rust = notices.parts.first { $0.heading.hasPrefix("The Rust libraries") }?.paragraphs ?? []
    check(rust.count > 50 && rust.contains { $0.hasPrefix("secp256k1-sys ") }
            && rust.contains { $0.hasPrefix("uniffi_core ") },
          "the notices name the \(rust.count) libraries in the Rust library, secp256k1-sys and UniFFI among them")
} else {
    check(false, "the license notices were given (the first argument)")
}

print(failed == 0 ? "All checks passed." : "\(failed) check(s) FAILED.")
exit(failed == 0 ? 0 : 1)
