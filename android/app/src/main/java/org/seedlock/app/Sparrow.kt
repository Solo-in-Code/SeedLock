package org.seedlock.app

import android.view.View
import org.seedlock.ffi.AddressType
import org.seedlock.ffi.Net
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.Wallet
import org.seedlock.ffi.qrCode

/** Opens the backup `name` and lays its wallet out for Sparrow (the command-line program's menu 5). */
fun sparrowFlow(app: MainActivity, name: String, text: String) {
    app.choose(
        "Wallet for Sparrow",
        "Is this wallet for real bitcoin, or for testing?",
        listOf("Real bitcoin (mainnet)", "Testing (testnet or signet)")
    ) { choice ->
        val net = if (choice == 0) Net.MAINNET else Net.TESTNET
        unlock(app, name, text, true) { seed, passphrase ->
            app.busy("Opening the wallet...", {
                try {
                    seed.wallet(passphrase, net)
                } finally {
                    passphrase.wipe()
                    seed.close()
                }
            }) { result ->
                result.fold(
                    onSuccess = { wallet -> app.open(SparrowScreen(app, wallet, name)) },
                    onFailure = { e -> app.say("Not opened", e.toUserText()) }
                )
            }
        }
    }
}

/**
 * The wallet laid out for Sparrow: one QR code to scan (which names the keystore after the backup,
 * `name`), or three fields to type; and addresses to compare. Or its silent payment wallet.
 */
class SparrowScreen(app: MainActivity, private var wallet: Wallet?, private val name: String) : Screen(app) {
    override val title = "Wallet for Sparrow"
    private var type = AddressType.NATIVE_SEGWIT
    private var silent = false

    override fun build(): View {
        val page = Page(app)
        val w = wallet ?: return page.view
        if (silent) return silentPayments(page, w)
        val s = try {
            w.sparrow(type, name)
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
            return page.view
        }
        page.text("Wallet ${s.fingerprint}: ${s.addressType}, ${s.network}.")
        if (s.passphraseUsed) page.small("With its BIP39 passphrase: a different wallet from the one without it.")

        page.heading("In one step: scan this QR code")
        page.small(
            "In Sparrow: File > Import Wallet, then Output Descriptor and Scan..., and hold this phone's screen toward " +
                "the computer's camera. Sparrow names the wallet's keystore \"${s.label}\". When it asks for the " +
                "wallet's name, type \"$name\" there too."
        )
        try {
            page.qr(qrCode(s.qr))
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
        }
        page.small("The same, as text:")
        page.code(s.descriptor)

        page.heading("Or type the three fields")
        page.small("In Sparrow: File > New Wallet, Script Type \"${s.addressType}\", Keystore \"Watch Only Wallet\":")
        page.text("Master fingerprint:")
        page.code(s.fingerprint)
        page.text("Derivation:")
        page.code(s.derivation)
        page.text("xpub:")
        page.code(s.xpub)

        page.heading("First receive addresses")
        page.small("Sparrow must show exactly these.")
        for (address in s.firstAddresses) {
            page.small(address.path)
            page.code(address.address)
        }

        page.heading("Another address type?")
        page.small("Only if your wallet in Sparrow uses another Script Type:")
        for (kind in AddressType.entries) {
            if (kind == type) continue
            page.button(sparrowName(kind)) {
                type = kind
                app.refresh()
            }
        }

        page.heading("A silent payment wallet?")
        page.small(
            "One address that you give to everyone, and nobody else can tell which payments on the blockchain went " +
                "to it. In Sparrow 2.5 or later."
        )
        page.button("Silent payment wallet") {
            silent = true
            app.refresh()
        }
        return page.view
    }

    /** The silent payment wallet: the address to give to payers, and the watch-only wallet that finds the payments. */
    private fun silentPayments(page: Page, w: Wallet): View {
        val s = try {
            w.silentPayments(name)
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
            return page.view
        }
        page.text("Silent payment wallet ${s.fingerprint}, ${s.network}.")
        if (s.passphraseUsed) page.small("With its BIP39 passphrase: a different wallet from the one without it.")

        page.heading("Your silent payment address")
        page.small(
            "Give it to anyone who pays you, as often as you like: each payment gets an address of its own on the " +
                "blockchain, which nobody else can link to this one or to the other payments."
        )
        page.code(s.address)

        page.heading("In one step: scan this QR code")
        page.small(
            "In Sparrow 2.5 or later: File > Import Wallet, then Output Descriptor and Scan..., and hold this phone's " +
                "screen toward the computer's camera."
        )
        try {
            page.qr(qrCode(s.descriptor))
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
        }
        page.small("The same, as text:")
        page.code(s.descriptor)

        page.heading("Or type the fields")
        page.small("In Sparrow: File > New Wallet, Policy Type \"$SPARROW_SP_POLICY\", Keystore \"Watch Only Wallet\":")
        page.text("Master fingerprint:")
        page.code(s.fingerprint)
        page.text("Derivation:")
        page.code(s.derivation)
        page.text("${s.spscanName}:")
        page.code(s.spscan)
        page.small(
            "When Sparrow asks for the wallet's name, type \"$name\". Sparrow calls the keystore \"Keystore 1\": it " +
                "takes no name from a silent payment wallet's descriptor. To give it the backup's name too: the wallet's " +
                "Settings tab, the keystore's Label field, \"${s.label}\" (Sparrow allows 16 characters), then Apply."
        )

        page.heading("Keep it private")
        page.small(
            "With this descriptor Sparrow FINDS the payments to your address. It cannot spend them: that takes your " +
                "seed, which stays on this phone. But whoever has it sees every payment you receive. To find them, " +
                "Sparrow must be connected to a server that supports silent payments (not to Bitcoin Core), and it asks " +
                "from which date to look: the day you first gave out the address. To spend them, Sparrow sends the " +
                "transaction to this phone to be signed, as for any other wallet."
        )
        page.button("Back to the usual wallet") {
            silent = false
            app.refresh()
        }
        return page.view
    }

    override fun shown() {
        app.qrShowing(true)
    }

    override fun hidden() {
        app.qrShowing(false)
    }

    override fun closed() {
        wallet?.close()
        wallet = null
    }
}

/** How Sparrow calls a silent payment wallet (its Policy Type). */
private const val SPARROW_SP_POLICY = "Single Signature SP (Silent Payments)"

/** Sparrow's name for an address type (the buttons' labels). */
private fun sparrowName(type: AddressType): String = when (type) {
    AddressType.NATIVE_SEGWIT -> "Native Segwit (P2WPKH)"
    AddressType.NESTED_SEGWIT -> "Nested Segwit (P2SH-P2WPKH)"
    AddressType.TAPROOT -> "Taproot (P2TR)"
    AddressType.LEGACY -> "Legacy (P2PKH)"
}
