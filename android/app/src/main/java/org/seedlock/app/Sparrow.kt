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
 * `name`), or three fields to type; and addresses to compare.
 */
class SparrowScreen(app: MainActivity, private var wallet: Wallet?, private val name: String) : Screen(app) {
    override val title = "Wallet for Sparrow"
    private var type = AddressType.NATIVE_SEGWIT

    override fun build(): View {
        val page = Page(app)
        val w = wallet ?: return page.view
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

/** Sparrow's name for an address type (the buttons' labels). */
private fun sparrowName(type: AddressType): String = when (type) {
    AddressType.NATIVE_SEGWIT -> "Native Segwit (P2WPKH)"
    AddressType.NESTED_SEGWIT -> "Nested Segwit (P2SH-P2WPKH)"
    AddressType.TAPROOT -> "Taproot (P2TR)"
    AddressType.LEGACY -> "Legacy (P2PKH)"
}
