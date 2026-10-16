// Calls SeedLock's Rust bridge (seedlock-ffi) through the generated Kotlin code and JNA, the way the
// Android app does, but on a computer (in the JVM). Run by android/jvm-check/run.sh, which also makes
// the transactions and the multisig wallet (with embit, an independent Python library) and checks the
// signatures afterwards.

import org.seedlock.ffi.AddressType
import org.seedlock.ffi.BackupStrength
import org.seedlock.ffi.MultisigKind
import org.seedlock.ffi.MultisigWallet
import org.seedlock.ffi.Net
import org.seedlock.ffi.NetworkHint
import org.seedlock.ffi.QrImage
import org.seedlock.ffi.QrScanner
import org.seedlock.ffi.RegisteredMultisig
import org.seedlock.ffi.ScanTarget
import org.seedlock.ffi.Seed
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.Transaction
import org.seedlock.ffi.backupInfo
import org.seedlock.ffi.backupNameIn
import org.seedlock.ffi.backupQrText
import org.seedlock.ffi.backupText
import org.seedlock.ffi.checkBackupName
import org.seedlock.ffi.generatePassword
import org.seedlock.ffi.qrAnimation
import org.seedlock.ffi.qrCode
import org.seedlock.ffi.seedlockVersion
import org.seedlock.ffi.wordSuggestions
import java.io.File
import kotlin.system.exitProcess

private var failures = 0

private fun check(ok: Boolean, what: String) {
    println((if (ok) "OK   " else "FAIL ") + what)
    if (!ok) failures++
}

/** A password's bytes, as the app passes them (it wipes its copy right after the call). */
private fun bytes(text: String): ByteArray = text.toByteArray(Charsets.UTF_8)

/** A QR code drawn as a camera would see it: 6 pixels per module (fewer for a large code), a light border, on a grey picture. */
private fun picture(code: QrImage): Triple<Int, Int, ByteArray> {
    val (w, h) = Pair(640, 480)
    val px = (h / (code.width + 8)).coerceIn(4, 6)
    val side = (code.width + 8) * px
    val x0 = (w - side) / 2
    val y0 = (h - side) / 2
    val luma = ByteArray(w * h) { 170.toByte() }
    for (y in 0 until side) {
        for (x in 0 until side) {
            val mx = x / px - 4
            val my = y / px - 4
            val dark = mx in 0 until code.width && my in 0 until code.width && code.modules[my * code.width + mx].toInt() == 1
            luma[(y0 + y) * w + x0 + x] = (if (dark) 35 else 230).toByte()
        }
    }
    return Triple(w, h, luma)
}

fun main(args: Array<String>) {
    val dir = File(args[0])
    val testWords = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    check(seedlockVersion().isNotEmpty(), "the library answers (SeedLock ${seedlockVersion()})")

    // 1. A known seed: fingerprint, and the wallet laid out for Sparrow (BIP84's test vector)
    Seed.fromWords(testWords).use { seed ->
        check(seed.fingerprint() == "73c5da0a" && seed.wordCount() == 12, "the BIP39 test seed: fingerprint 73c5da0a, 12 words")
        seed.wallet(ByteArray(0), Net.MAINNET).use { wallet ->
            val s = wallet.sparrow(AddressType.NATIVE_SEGWIT, "Poupança da família")
            check(s.label == "Poupança da famí" && s.qr.startsWith("UR:CRYPTO-OUTPUT/"), "the QR code for Sparrow names the keystore after the backup: ${s.label}")
            check(s.derivation == "m/84'/0'/0'" && s.xpub.startsWith("xpub6CatWdiZiodmUeTDp8LT5or8nmbKNcuyvz7WyksVFkKB4RHwCD3Xyuv"), "Sparrow's fields: derivation and xpub")
            check(s.firstAddresses[0].address == "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu", "the first receive address")
            check(s.descriptor.startsWith("wpkh([73c5da0a/84h/0h/0h]xpub") && s.otherTypes.size == 3, "the one-line descriptor, and the 3 other address types")
            val code = qrCode(s.descriptor)
            check(code.width >= 21 && code.modules.size == code.width * code.width, "the descriptor's QR code: ${code.width} x ${code.width} modules")
        }
        try {
            seed.wallet(bytes("TREZOR"), Net.TESTNET).use { other ->
                check(other.fingerprint() != "73c5da0a" && other.sparrow(AddressType.TAPROOT, "x").xpub.startsWith("tpub"), "with a passphrase, on a test network: another wallet, tpub")
            }
        } catch (e: SeedLockException) {
            check(false, "passphrase wallet: ${e.message}")
        }
    }

    // 2. Errors arrive as SeedLockException, with the message to show
    try {
        Seed.fromWords(testWords.replace("about", "abaut")).close()
        check(false, "a wrong word is refused")
    } catch (e: SeedLockException.Failed) {
        check(e.message!!.contains("#12"), "a wrong word is refused, saying which: ${e.message}")
    }
    check(checkBackupName(" my wallet.seedlock ") == "my wallet" && wordSuggestions("zo").contains("zoo"), "helpers: backup names, word suggestions")
    check(generatePassword().split("-").size == 8, "a generated password has 8 words")

    // 3. A seed from embit's words: a backup made, opened with its password, refused with another
    val seed = Seed.fromWords(File(dir, "words.txt").readText().trim())
    check(seed.wordCount() == 24, "the 24 words from embit")
    val password = bytes("correct horse battery staple")
    val backup = seed.encrypt(password.copyOf(), BackupStrength.LIGHT)
    check(backup.text.startsWith("SEEDLOCK1:") && backup.fileContents.contains(backup.text), "backup made: its file and its QR text")
    check(backupInfo(backup.text).memoryMib == 64, "read without the password: it takes 64 MiB to open")
    try {
        Seed.unlock(backup.fileContents, bytes("not the password")).close()
        check(false, "a wrong password is refused")
    } catch (e: SeedLockException.WrongPassword) {
        check(true, "a wrong password is refused as WrongPassword: ${e.message}")
    }
    Seed.unlock(backup.fileContents, password).use { opened -> check(opened.sameAs(seed), "the backup opens to the same seed") }

    // 3b. The backup's QR code carries its name; the camera scanner reads it as the backup all the same
    val named = backupQrText(backup.fileContents, "Poupança")
    QrScanner(ScanTarget.BACKUP).use { scanner ->
        val (w, h, luma) = picture(qrCode(named))
        val read = if (scanner.addPicture(w, h, luma).done) scanner.backupText() else null
        check(read != null && backupText(read) == backup.text && backupNameIn(read) == "Poupança", "the backup's QR code, with its name, read by the scanner")
    }

    // 4. The wallet for Sparrow as its QR code, which embit and urtypes check; then the transaction
    //    embit made: read from its file, reviewed, signed and written back
    seed.wallet(ByteArray(0), Net.MAINNET).use { wallet ->
        File(dir, "sparrow.txt").writeText(wallet.sparrow(AddressType.NATIVE_SEGWIT, "embit check").qr)
    }
    val tx = Transaction.fromBytes(File(dir, "in.psbt").readBytes())
    check(tx.networkHint() == NetworkHint.MAINNET && tx.inputCount() == 1 && tx.outputCount() == 2, "transaction read from a file: mainnet, 1 input, 2 outputs")
    seed.wallet(ByteArray(0), Net.MAINNET).use { wallet ->
        val r = wallet.review(tx, emptyList())
        check(r.inputs[0].mine && r.outputs[0].external && !r.outputs[1].external, "review: the input is ours, output 0 goes to someone else, output 1 is change")
        check(r.fee == "0.00001000" && r.sending == "0.00060000" && r.inputsToSign == 1, "review: fee 0.00001000, leaving 0.00060000, 1 input to sign")
        val signed = wallet.sign(tx, emptyList())
        check(signed.inputs == 1, "signed 1 input")
        File(dir, "signed.b64").writeText(signed.transaction.toBase64())

        // 5. The signed transaction as an animated QR code, read back by the camera scanner
        val codes = qrAnimation(signed.transaction.qrFrames())
        check(codes.size >= 2 && codes.all { it.width == codes[0].width }, "${codes.size} animated QR codes, all ${codes[0].width} modules wide")
        QrScanner(ScanTarget.TRANSACTION).use { scanner ->
            var done = false
            for (i in 0 until codes.size * 2) {
                val (w, h, luma) = picture(codes[(i + 1) % codes.size])
                if (scanner.addPicture(w, h, luma).done) {
                    done = true
                    break
                }
            }
            check(done && scanner.transaction()!!.toBase64() == signed.transaction.toBase64(), "read back through the scanner: the same signed transaction")
        }
    }
    seed.close()

    // 6. Multisig: a 2-of-3 wallet that embit described the way Sparrow exports it, read and kept as the
    //    command-line program keeps it; a cosigner's key for it; its payment checked and signed by two of
    //    its cosigners, one after the other. embit checks both signatures, and urtypes the key's QR code.
    val cosigners = File(dir, "ms_words.txt").readLines().filter { it.isNotBlank() }
    val ms = MultisigWallet.fromText(File(dir, "ms_descriptor.txt").readText())
    check(ms.label() == "2 of 3" && ms.kind() == MultisigKind.NATIVE_SEGWIT && !ms.mainnet(), "multisig wallet read from Sparrow's export: ${ms.label()}, ${ms.kindName()}, ${ms.networkName()}")
    val first = File(dir, "ms_first.txt").readText().trim()
    check(ms.receiveAddress(0) == first, "its first receive address is embit's: $first")
    val kept = ms.fileContents("vault")
    check(
        kept.startsWith("# SeedLock multisig wallet \"vault\": 2 of 3, Native Segwit (P2WSH), a test network (testnet or signet)\n") &&
            MultisigWallet.fromText(kept).sameAs(ms),
        "kept as the command-line program's vault.multisig, which reads back as the same wallet"
    )
    QrScanner(ScanTarget.MULTISIG_WALLET).use { scanner ->
        val (w, h, luma) = picture(qrCode(ms.descriptor()))
        check(scanner.addPicture(w, h, luma).done && scanner.multisigWallet().sameAs(ms), "its descriptor's QR code, read by the camera scanner")
    }
    val registered = listOf(RegisteredMultisig("vault", ms))
    val msTx = Transaction.fromBytes(File(dir, "ms.psbt").readBytes())
    check(msTx.networkHintWith(registered) == NetworkHint.TEST_NETWORK, "the multisig payment is for a test network")
    check(msTx.inputFingerprints().sorted() == ms.cosigners().map { it.fingerprint }.sorted(), "it names its three cosigners' fingerprints, which tell which backup signs it")
    var partly: Transaction? = null
    Seed.fromWords(cosigners[0]).use { a ->
        a.wallet(ByteArray(0), Net.TESTNET).use { wallet ->
            val key = wallet.cosignerKeyIn(ms, "alice")
            val own = ms.cosigners()[ms.positionOf(wallet.fingerprint())!! - 1]
            check(key.derivation == "m/48'/1'/0'/2'" && key.xpub == own.xpub && key.label == "alice", "the first cosigner's key for the wallet: m/48'/1'/0'/2', the wallet's own tpub, named \"alice\"")
            check(wallet.cosignerKey(MultisigKind.NATIVE_SEGWIT, false, "alice") == key, "the same key when its kind and network are chosen by hand")
            File(dir, "cosigner.txt").writeText(key.qr)
            try {
                wallet.review(msTx, emptyList())
                check(false, "a multisig payment of a wallet that is not on the phone is refused")
            } catch (e: SeedLockException) {
                check(e.message!!.contains("Add the wallet to this phone first"), "a wallet that is not on the phone: refused, saying how to add it")
            }
            val r = wallet.review(msTx, registered)
            check(
                r.inputs[0].owner.startsWith("yours, from the multisig wallet \"vault\"") && r.outputs[1].note.startsWith("change back to the multisig wallet \"vault\"") && r.outputs[0].external,
                "review: the input is the wallet's, its change is verified from all its cosigners' keys, output 0 leaves it"
            )
            check(r.multisig!!.endsWith("1 more cosigner must sign it too before Sparrow can finalize it."), "review: after this seed, 1 more cosigner must sign")
            val signed = wallet.sign(msTx, registered)
            check(
                signed.inputs == 1 && signed.multisig == "Multisig \"vault\": it now has 1 of the 2 signatures it needs. 1 more cosigner must sign it too before Sparrow can finalize it.",
                "the first cosigner signed its part: it has 1 of the 2 signatures it needs now"
            )
            partly = signed.transaction
        }
    }
    Seed.fromWords(cosigners[1]).use { b ->
        b.wallet(ByteArray(0), Net.TESTNET).use { wallet ->
            val r = wallet.review(partly!!, registered)
            check(r.multisig!!.endsWith("That is enough: after this, Sparrow can finalize and broadcast the transaction."), "the second cosigner's review: its signature is the last one needed")
            val both = wallet.sign(partly!!, registered)
            check(both.multisig == "Multisig \"vault\": it now has the 2 signatures it needs. Sparrow can finalize and broadcast it.", "after the second cosigner: it has the 2 signatures it needs")
            File(dir, "ms_signed.b64").writeText(both.transaction.toBase64())
        }
    }
    if (failures > 0) exitProcess(1)
}
