package org.seedlock.app

import android.view.View
import org.seedlock.ffi.CosignerKey
import org.seedlock.ffi.MultisigKind
import org.seedlock.ffi.MultisigWallet
import org.seedlock.ffi.Net
import org.seedlock.ffi.ScanTarget
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.checkWalletName
import org.seedlock.ffi.multisigKindName
import org.seedlock.ffi.qrCode

// Multisig wallets, as the command-line program's menu 10 has them: this seed's key for a multisig wallet
// in Sparrow, the multisig wallets kept on this phone, and (in Sign.kt) signing their transactions.

/** Where Sparrow shows a multisig wallet's QR code, and saves its file. */
private const val FROM_SPARROW =
    "Sparrow gives the wallet as a QR code (the wallet's Settings tab: under Script Policy, the QR code button) or as " +
        "a file (File > Export Wallet > Output Descriptor). It holds no secrets, only the wallet's public keys."

/** Adds a multisig wallet that was made in Sparrow (the command-line program's menu 10, 2). */
fun addMultisig(app: MainActivity) {
    app.choose(
        "Add a multisig wallet",
        "Once a multisig wallet (for example 2 of 3) is made in Sparrow, add it here: then the backups on this phone " +
            "that are its cosigners can check and sign its transactions. $FROM_SPARROW",
        listOf("Scan Sparrow's QR code", "Open the file that Sparrow exported", "Paste its descriptor")
    ) { choice ->
        when (choice) {
            0 -> app.open(ScanScreen(app, ScanTarget.MULTISIG_WALLET) { scanner ->
                val read = try {
                    Result.success(scanner.multisigWallet())
                } catch (e: Exception) {
                    Result.failure(e)
                }
                scanner.close()
                app.back()
                read.fold(onSuccess = { checkMultisig(app, it) }, onFailure = { app.say("Not added", it.toUserText()) })
            })
            1 -> app.pickFile { uri ->
                if (uri == null) return@pickFile
                val wallet = try {
                    MultisigWallet.fromText(String(readFile(app, uri, 1_000_000, "a wallet's descriptor"), Charsets.UTF_8))
                } catch (e: Exception) {
                    app.say("Not added", e.toUserText())
                    return@pickFile
                }
                checkMultisig(app, wallet)
            }
            else -> app.open(PasteMultisigScreen(app))
        }
    }
}

/** A multisig wallet that was read: unless it is already on this phone, it is shown for checking before it is added. */
private fun checkMultisig(app: MainActivity, wallet: MultisigWallet) {
    val same = app.multisig.same(wallet)
    if (same != null) {
        app.say("Already on this phone", "This multisig wallet is already on this phone, as \"$same\". Nothing was changed.") {
            app.home()
            app.open(MultisigScreen(app, same))
        }
        return
    }
    app.open(AddMultisigScreen(app, wallet))
}

/** A multisig wallet's descriptor typed or pasted in. */
class PasteMultisigScreen(app: MainActivity) : Screen(app) {
    override val title = "Paste a multisig wallet"

    override fun build(): View {
        val page = Page(app)
        page.text(
            "Paste the wallet's output descriptor (it starts with wsh(, sh(wsh( or sh( and ends with # and 8 characters), " +
                "or the whole contents of the file that Sparrow exported."
        )
        val field = page.field("wsh(sortedmulti(...", FieldKind.TEXT)
        page.button("Add this wallet") {
            val text = field.text.toString()
            if (text.isBlank()) return@button
            val wallet = try {
                MultisigWallet.fromText(text)
            } catch (e: SeedLockException) {
                app.say("Not added", e.toUserText())
                return@button
            }
            checkMultisig(app, wallet)
        }
        return page.view
    }
}

/**
 * The cosigners of a multisig wallet, in its descriptor's order (with their keys when `withKeys`), each
 * with the backups on this phone that have its fingerprint (see [Backups.fingerprint]).
 */
private fun showCosigners(app: MainActivity, page: Page, wallet: MultisigWallet, withKeys: Boolean) {
    page.heading("Cosigners (${wallet.cosignerCount()})")
    var unknown = false
    wallet.cosigners().forEachIndexed { i, c ->
        page.text("${i + 1})  ${c.fingerprint}   ${c.derivation}")
        val here = app.backups.withFingerprint(c.fingerprint)
        if (here.isNotEmpty()) page.good("Backup on this phone: ${here.joinToString(" or ") { "\"$it\"" }}") else unknown = true
        if (withKeys) page.small(c.xpub)
    }
    if (unknown) {
        page.small(
            "A backup on this phone shows here once it was opened (a backup used with a BIP39 passphrase is another " +
                "wallet, with another fingerprint, which SeedLock does not keep)."
        )
    }
}

/** The backups on this phone that are cosigners of `wallet`, as (position, backup name), in its order. */
private fun cosignersHere(app: MainActivity, wallet: MultisigWallet): List<Pair<Int, String>> =
    wallet.cosigners().flatMapIndexed { i, c -> app.backups.withFingerprint(c.fingerprint).map { Pair(i + 1, it) } }

/**
 * A multisig wallet read from Sparrow, to check before it is added (as the command-line program asks): its
 * kind, its cosigners, and its first receive address, which Sparrow must show too.
 */
class AddMultisigScreen(app: MainActivity, private val wallet: MultisigWallet) : Screen(app) {
    override val title = "Add a multisig wallet"

    override fun build(): View {
        val page = Page(app)
        page.text("Multisig wallet: ${wallet.label()} signatures needed, ${wallet.kindName()}, ${wallet.networkName()}.")
        showCosigners(app, page, wallet, true)
        page.heading("Its first receive address")
        val first = try {
            wallet.receiveAddress(0)
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
            return page.view
        }
        page.code(first)
        page.small("Sparrow must show exactly this one: in the wallet's Addresses tab, the first receive address.")
        page.heading("Does it match?")
        page.button("It matches, and these are the right cosigners: add it") { add() }
        page.button("Cancel: do not add it") { app.back() }
        return page.view
    }

    private fun add() {
        val prompt = "The wallet's name on this phone (it also names its file when you save a copy):"
        askName(app, prompt, wallet.defaultName(), ::checkWalletName, app.multisig) { name ->
            try {
                app.multisig.save(name, wallet.fileContents(name))
                app.home()
                app.open(MultisigScreen(app, name))
                app.say(
                    "Multisig wallet added",
                    "The multisig wallet \"$name\" is on this phone now. To sign one of its transactions, choose \"Sign " +
                        "a transaction\" in a backup that is one of its cosigners (this screen offers them, once they were " +
                        "opened on this phone): SeedLock checks every input of this wallet, and that the change really " +
                        "returns to it."
                )
            } catch (e: Exception) {
                app.say("Not saved", e.toUserText())
            }
        }
    }
}

/** A multisig wallet on this phone: what it is, a copy of its file, and deleting it. */
class MultisigScreen(app: MainActivity, private val name: String) : Screen(app) {
    override val title = name

    override fun build(): View {
        val page = Page(app)
        val wallet = try {
            app.multisig.wallet(name)
        } catch (e: Exception) {
            page.warning("This multisig wallet's file cannot be read: ${e.toUserText()}")
            removeIt(page)
            return page.view
        }
        page.text("Multisig wallet \"$name\": ${wallet.label()}, ${wallet.kindName()}, ${wallet.networkName()}.")
        showCosigners(app, page, wallet, false)
        page.heading("First receive address")
        try {
            page.code(wallet.receiveAddress(0))
            page.small("Sparrow shows the same one, in the wallet's Addresses tab.")
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
        }

        page.heading("Signing its transactions")
        page.small(
            "${wallet.threshold()} of the ${wallet.cosignerCount()} cosigners sign each payment, each with its own seed. " +
                "SeedLock checks every input of this wallet, and that the change really returns to it."
        )
        val here = cosignersHere(app, wallet)
        for ((position, backup) in here) {
            page.button("Sign a transaction with \"$backup\" (cosigner $position)") {
                val text = try {
                    app.backups.read(backup)
                } catch (e: Exception) {
                    app.say("Not read", e.toUserText())
                    return@button
                }
                SignFlow(app, backup, text).start()
            }
        }
        if (here.isEmpty()) {
            page.small(
                "Open a backup on this phone that is one of its cosigners and choose \"Sign a transaction\". (Once a " +
                    "backup was opened here, its fingerprint shows beside its name, and its button here.)"
            )
        }

        page.heading("Copy it")
        page.small(
            "No secrets in it, but anyone who reads it can see all the wallet's addresses. Sparrow opens the file " +
                "(File > Import Wallet > Output Descriptor), and SeedLock on a computer reads it in the \"multisig " +
                "wallets\" folder inside its \"seedlocker backups\" folder."
        )
        page.button("Save a copy as a file (${MultisigWallets.EXT})") {
            app.createFile("$name${MultisigWallets.EXT}", "application/octet-stream") { uri ->
                if (uri == null) return@createFile
                try {
                    writeFile(app, uri, app.multisig.read(name).toByteArray(Charsets.UTF_8))
                    app.say("Saved", "A copy of the multisig wallet \"$name\" is saved: ${describePlace(app, uri)}.")
                } catch (e: Exception) {
                    app.say("Not saved", e.toUserText())
                }
            }
        }
        page.small(PICKER_HINT)
        removeIt(page)
        return page.view
    }

    private fun removeIt(page: Page) {
        page.heading("Remove it")
        page.button("Delete it from this phone") {
            app.confirm(
                "Delete \"$name\"?",
                "The multisig wallet is deleted from this phone only: its coins stay where they are, and Sparrow keeps " +
                    "the wallet. To sign its transactions here again, add it again.",
                "Delete"
            ) {
                app.multisig.delete(name)
                app.home()
            }
        }
    }
}

/** What the backup's seed is for multisig: a cosigner of a wallet on this phone, or not yet. */
private sealed class Cosigning {
    /** A cosigner of the multisig wallet `wallet` on this phone, at `position` (1 for the first): its key there. */
    class Known(val wallet: String, val position: Int, val key: CosignerKey) : Cosigning()

    /** Not a cosigner of a wallet on this phone yet: its key for each kind of wallet, on each network. */
    class Choose(val keys: Map<Pair<MultisigKind, Boolean>, CosignerKey>) : Cosigning()
}

/**
 * Opens the backup `name` and lays its key out for a multisig wallet in Sparrow (the command-line
 * program's menu 10, 1). When the seed is a cosigner of a multisig wallet on this phone, that wallet's kind
 * and network are taken; otherwise the person chooses them. Only the public keys are kept meanwhile: the
 * seed is closed as soon as they are known.
 */
fun cosignerFlow(app: MainActivity, name: String, text: String) {
    val registered = app.multisig.all()
    unlock(app, name, text, true) { seed, passphrase ->
        app.busy("Opening the wallet...", {
            try {
                seed.wallet(passphrase, Net.MAINNET).use { wallet ->
                    val fingerprint = wallet.fingerprint()
                    val known = registered.firstOrNull { it.wallet.positionOf(fingerprint) != null }
                    if (known != null) {
                        Cosigning.Known(known.name, known.wallet.positionOf(fingerprint) ?: 0, wallet.cosignerKeyIn(known.wallet, name))
                    } else {
                        val keys = HashMap<Pair<MultisigKind, Boolean>, CosignerKey>()
                        for (kind in MultisigKind.entries) {
                            for (mainnet in listOf(true, false)) keys[Pair(kind, mainnet)] = wallet.cosignerKey(kind, mainnet, name)
                        }
                        Cosigning.Choose(keys)
                    }
                }
            } finally {
                passphrase.wipe()
                seed.close()
            }
        }) { result ->
            result.fold(
                onSuccess = { found ->
                    when (found) {
                        is Cosigning.Known -> app.open(
                            CosignerScreen(
                                app, name, found.key,
                                "This seed is cosigner ${found.position} of the multisig wallet \"${found.wallet}\" on this " +
                                    "phone: its kind and network are taken from there."
                            )
                        )
                        is Cosigning.Choose -> chooseKey(app, name, found.keys)
                    }
                },
                onFailure = { e -> app.say("Not opened", e.toUserText()) }
            )
        }
    }
}

/** Asks which network and which kind of multisig wallet the key is for (both questions of the command-line program). */
private fun chooseKey(app: MainActivity, name: String, keys: Map<Pair<MultisigKind, Boolean>, CosignerKey>) {
    app.choose(
        "Key for a multisig wallet",
        "Is the multisig wallet for real bitcoin, or for testing?",
        listOf("Real bitcoin (mainnet)", "Testing (testnet or signet)")
    ) { network ->
        val mainnet = network == 0
        val kinds = MultisigKind.entries
        app.choose(
            "Which kind of multisig wallet?",
            "Sparrow calls it the wallet's \"Script Type\". All its cosigners use the same one.",
            kinds.mapIndexed { i, kind -> multisigKindName(kind) + if (i == 0) ", the usual one" else "" }
        ) { k ->
            val key = keys[Pair(kinds[k], mainnet)] ?: return@choose
            app.open(CosignerScreen(app, name, key, null))
        }
    }
}

/**
 * This seed's key for a multisig wallet, laid out for Sparrow: one QR code to scan (which names the keystore
 * after the backup, `name`), or three fields to type. Only public keys are on this screen.
 */
class CosignerScreen(app: MainActivity, private val name: String, private val key: CosignerKey, private val note: String?) : Screen(app) {
    override val title = "Key for a multisig wallet"

    override fun build(): View {
        val page = Page(app)
        page.text("Seed ${key.fingerprint} as a cosigner of a multisig wallet: ${key.kindName}, ${key.network}.")
        note?.let { page.small(it) }

        page.heading("In one step: scan this QR code")
        val shortened = if (key.labelShortened) " (Sparrow allows 16 characters)" else ""
        page.small(
            "In Sparrow: File > New Wallet, Policy Type \"Multi Signature\" (for example 2 of 3), Script Type " +
                "\"${key.kindName}\". For this seed's keystore choose \"Watch Only Wallet\", then click the camera button " +
                "next to the xpub field (\"Scan a QR code\") and hold this phone's screen toward the computer's camera. " +
                "Sparrow fills in the fields and names the keystore \"${key.label}\"$shortened."
        )
        try {
            page.qr(qrCode(key.qr))
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
        }

        page.heading("Or type the three fields")
        page.small("In this seed's keystore (Watch Only Wallet):")
        page.text("Master fingerprint:")
        page.code(key.fingerprint)
        page.text("Derivation:")
        page.code(key.derivation)
        page.text("xpub:")
        page.code(key.xpub)
        page.small("In one piece, for wallets that ask for it that way:")
        page.code(key.keyWithOrigin)

        page.heading("Then")
        page.small(
            "When every cosigner's keystore is in and the wallet is saved in Sparrow, add the wallet to this phone (on " +
                "the first screen: \"Add a multisig wallet\"). Then SeedLock can check and sign its transactions. " +
                FROM_SPARROW
        )

        page.heading("Save it")
        page.small("To take it to the computer on an SD card or a USB stick. No secrets in it: only this seed's public key for the wallet.")
        page.button("Save its QR code as a picture (PNG)") { savePicture() }
        page.button("Save it as a text file (.txt)") {
            app.createFile("$name-cosigner.txt", "text/plain") { uri ->
                if (uri == null) return@createFile
                try {
                    writeFile(app, uri, key.text.toByteArray(Charsets.UTF_8))
                    app.say("Saved", "This seed's key is saved as text: ${describePlace(app, uri)}.")
                } catch (e: Exception) {
                    app.say("Not saved", e.toUserText())
                }
            }
        }
        page.small(PICKER_HINT)
        return page.view
    }

    private fun savePicture() {
        val picture = try {
            qrPicture(qrCode(key.qr), listOf("SeedLock cosigner key \"$name\" (${key.fingerprint})", "${key.kindName}, ${key.network}"))
        } catch (e: SeedLockException) {
            app.say("Not saved", e.toUserText())
            return
        }
        app.createFile("$name-cosigner-qr.png", "image/png") { uri ->
            if (uri == null) return@createFile
            try {
                writePng(app, uri, picture)
                app.say("Saved", "The QR code of this seed's key is saved as a picture: ${describePlace(app, uri)}.")
            } catch (e: Exception) {
                app.say("Not saved", e.toUserText())
            }
        }
    }

    override fun shown() {
        app.qrShowing(true)
    }

    override fun hidden() {
        app.qrShowing(false)
    }
}
