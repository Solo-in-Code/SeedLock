package org.seedlock.app

import android.view.View
import org.seedlock.ffi.Net
import org.seedlock.ffi.NetworkHint
import org.seedlock.ffi.RegisteredMultisig
import org.seedlock.ffi.ScanTarget
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.Signed
import org.seedlock.ffi.Transaction
import org.seedlock.ffi.TxReview
import org.seedlock.ffi.Wallet

/**
 * Signing a transaction with the backup `name`, as the command-line program's menu 6 does: the
 * transaction arrives (camera or file), its network is found, the backup is opened, everything is
 * shown for review, and only then is it signed and given back to Sparrow. The multisig wallets on this
 * phone are known throughout, so that the seed can sign as one of their cosigners.
 */
class SignFlow(private val app: MainActivity, private val name: String, private val text: String) {
    fun start() {
        app.choose(
            "Sign a transaction",
            "The payment is prepared in Sparrow on your online computer. This phone checks it, shows you every " +
                "address and amount, and signs it only if you say yes. How does the transaction get to this phone?",
            listOf("Scan the QR code that Sparrow shows", "Open the .psbt file that Sparrow saved")
        ) { choice -> if (choice == 0) scan() else fromFile() }
    }

    private fun scan() {
        app.open(ScanScreen(app, ScanTarget.TRANSACTION) { scanner ->
            val tx = scanner.transaction()
            scanner.close()
            app.back()
            if (tx != null) received(tx)
        })
    }

    private fun fromFile() {
        app.pickFile { uri ->
            if (uri == null) return@pickFile
            val tx = try {
                Transaction.fromBytes(readFile(app, uri, 16 * 1024 * 1024, "a transaction"))
            } catch (e: Exception) {
                app.say("Not a transaction", e.toUserText())
                return@pickFile
            }
            received(tx)
        }
    }

    /**
     * Before the password is asked for: whether this backup is one of the wallets the transaction is for.
     * Its fingerprint is known once it was opened on this phone; when it is not among the transaction's,
     * the person learns which backup on this phone is, and may still open this one (with a BIP39
     * passphrase it is another wallet, with another fingerprint).
     */
    private fun received(tx: Transaction) {
        val ours = app.backups.fingerprint(name)
        val signers = tx.inputFingerprints()
        if (ours == null || signers.isEmpty() || ours in signers) {
            network(tx)
            return
        }
        val here = signers.flatMap { app.backups.withFingerprint(it) }.distinct()
        val which = if (here.isEmpty()) "" else "\n\nThe backup for it on this phone: ${here.joinToString(" or ") { "\"$it\"" }}."
        app.confirm(
            "Another backup?",
            "This transaction is for the wallet${if (signers.size == 1) "" else "s"} ${signers.joinToString(", ")}. This " +
                "backup is the wallet $ours, so it cannot sign it, unless you use it with a BIP39 passphrase (then it " +
                "is another wallet).$which",
            "Open this backup anyway"
        ) { network(tx) }
    }

    /**
     * The network decides which wallet signs: the transaction's key paths say it (or, for a legacy
     * multisig wallet's, the multisig wallet on this phone that it belongs to), or the person is asked.
     */
    private fun network(tx: Transaction) {
        val registered = app.multisig.all()
        val why = when (tx.networkHintWith(registered)) {
            NetworkHint.MAINNET -> return open(tx, Net.MAINNET, registered)
            NetworkHint.TEST_NETWORK -> return open(tx, Net.TESTNET, registered)
            NetworkHint.UNKNOWN -> "This transaction does not say which network it is for."
            NetworkHint.CONFLICTING -> "Some key paths in this transaction say mainnet and others a test network: something is wrong with it."
        }
        app.choose("Which network?", "$why Which wallet should sign it?", listOf("Real bitcoin (mainnet)", "A test network (testnet or signet)")) { choice ->
            open(tx, if (choice == 0) Net.MAINNET else Net.TESTNET, registered)
        }
    }

    private fun open(tx: Transaction, net: Net, registered: List<RegisteredMultisig>) {
        unlock(app, name, text, true) { seed, passphrase ->
            app.busy("Checking the transaction against your wallet...", {
                try {
                    val wallet = seed.wallet(passphrase, net)
                    try {
                        Pair(wallet, wallet.review(tx, registered))
                    } catch (e: Exception) {
                        wallet.close()
                        throw e
                    }
                } finally {
                    passphrase.wipe()
                    seed.close() // the wallet is all that is needed from here on
                }
            }) { result ->
                result.fold(
                    onSuccess = { (wallet, review) -> app.open(ReviewScreen(app, wallet, tx, review, registered)) },
                    onFailure = { e -> app.say("This transaction cannot be signed", e.toUserText()) }
                )
            }
        }
    }
}

/**
 * Everything to look at before signing; the transaction is signed only from here, on request (`registered`:
 * the multisig wallets on this phone, as for the review).
 */
class ReviewScreen(
    app: MainActivity,
    private var wallet: Wallet?,
    private val tx: Transaction,
    private val review: TxReview,
    private val registered: List<RegisteredMultisig>,
) : Screen(app) {
    override val title = "Check before signing"

    override fun build(): View {
        val page = Page(app)
        page.text("Network: ${review.network}      Wallet fingerprint: ${review.fingerprint}")
        review.multisig?.let { page.note(it) }
        page.heading("Inputs (${review.inputs.size})")
        for (input in review.inputs) page.text("#${input.index}   ${input.amount} BTC   ${input.owner}")
        page.heading("Outputs (${review.outputs.size})")
        for (output in review.outputs) {
            page.text("#${output.index}   ${output.amount} BTC   to:")
            page.code(output.address)
            if (output.external) page.warning(output.note) else page.small(output.note)
        }
        page.heading("Totals")
        page.text(
            "Total in: ${review.totalIn} BTC\nTotal out: ${review.totalOut} BTC\nFee: ${review.fee} BTC\n" +
                "Leaving your wallet: ${review.sending} BTC"
        )
        if (review.warnings.isNotEmpty()) {
            page.heading("WARNINGS")
            for (warning in review.warnings) page.warning(warning)
        }
        if (review.inputsToSign == 0) {
            page.warning("This wallet has nothing to sign in this transaction. Is it the right backup, passphrase and network?")
            if (app.multisig.names().isNotEmpty()) {
                page.small("For a multisig wallet's payment: the multisig wallet's screen shows which backups on this phone are its cosigners.")
            }
        } else {
            val inputs = if (review.inputsToSign == 1) "1 input" else "${review.inputsToSign} inputs"
            page.button("SIGN this transaction ($inputs)") { sign() }
        }
        page.button("Cancel: do not sign") { app.back() }
        return page.view
    }

    private fun sign() {
        val w = wallet ?: return
        app.busy("Signing...", { w.sign(tx, registered) }) { result ->
            result.fold(
                onSuccess = { signed -> app.replace(signedScreen(app, signed)) },
                onFailure = { e -> app.say("Not signed", e.toUserText()) }
            )
        }
    }

    override fun closed() {
        wallet?.close()
        wallet = null
    }
}

/** The signed transaction as an animated QR code for Sparrow, with the choice of a file instead. */
private fun signedScreen(app: MainActivity, signed: Signed): Screen {
    val tx = signed.transaction
    val frames = try {
        tx.qrFrames()
    } catch (e: SeedLockException) {
        emptyList()
    }
    val inputs = if (signed.inputs == 1) "1 input" else "${signed.inputs} inputs"
    return QrScreen(
        app, "Signed", frames, true,
        "Signed $inputs. In Sparrow, on the transaction's screen, click Scan QR and hold this phone's screen toward " +
            "the computer's camera. The code changes all the time: that is normal.",
        signed.multisig
    ) { page ->
        page.button("Save it as a file instead (.psbt)") {
            app.createFile("signed-${tx.txid().take(8)}.psbt", "application/octet-stream") { uri ->
                if (uri == null) return@createFile
                try {
                    writeFile(app, uri, tx.toBytes())
                    app.say(
                        "Saved",
                        "The signed transaction is saved: ${describePlace(app, uri)}. In Sparrow: File > Open Transaction, then Broadcast."
                    )
                } catch (e: Exception) {
                    app.say("Not saved", e.toUserText())
                }
            }
        }
        page.small(PICKER_HINT)
        page.button("Done") { app.back() }
    }
}
