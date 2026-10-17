package org.seedlock.app

import android.app.AlertDialog
import android.view.View
import android.widget.CheckBox
import android.widget.EditText
import org.seedlock.ffi.Seed
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.backupInfo

/**
 * Asks for the password of the backup `name` (and, when `withPassphrase`, whether the wallet uses a
 * BIP39 passphrase, and which), opens the backup on the background thread and hands over the seed and
 * the passphrase's bytes (empty when none). A wrong password asks again. Whoever receives the seed
 * closes it, and wipes the passphrase's bytes. The backup's wallet fingerprint (without a passphrase) is
 * remembered, to show it beside the backup's name from then on. While the prompt is open, SeedLock may be
 * left for a few minutes to copy the password from a password manager on this phone (see
 * [MainActivity.promptsForPassword]).
 */
fun unlock(app: MainActivity, name: String, text: String, withPassphrase: Boolean, then: (Seed, ByteArray) -> Unit) {
    val info = try {
        backupInfo(text)
    } catch (e: SeedLockException) {
        app.say("Damaged backup", e.toUserText())
        return
    }
    val page = Page(app)
    page.text("Password of the backup \"$name\":")
    val password = page.field("Password", FieldKind.PASSWORD)
    val uses: CheckBox? = if (withPassphrase) page.checkBox("This wallet uses a BIP39 passphrase") else null
    val passphrase: EditText? =
        if (withPassphrase) page.field("BIP39 passphrase (upper and lower case count)", FieldKind.PASSWORD) else null
    if (uses != null && passphrase != null) {
        passphrase.visibility = View.GONE
        uses.setOnCheckedChangeListener { _, checked -> passphrase.visibility = if (checked) View.VISIBLE else View.GONE }
    }
    page.small(
        "With a password manager on this phone: copy the password there and come back within " +
            "${MainActivity.KEEP_OPEN_MS / 60_000} minutes to paste it here. SeedLock empties the clipboard afterwards."
    )
    val dialog = app.showSecure(AlertDialog.Builder(app)
        .setTitle("Open the backup")
        .setView(page.view)
        .setPositiveButton("Open") { _, _ ->
            val secret = password.takeSecret()
            val pass = if (uses != null && uses.isChecked && passphrase != null) passphrase.takeSecret() else ByteArray(0)
            passphrase?.text?.clear()
            app.busy("Opening the backup (Argon2id, ${info.memoryMib} MiB): this takes a few seconds...", {
                val seed = try {
                    Seed.unlock(text, secret)
                } finally {
                    secret.wipe()
                }
                val fingerprint = try {
                    seed.fingerprint()
                } catch (e: Exception) {
                    null
                }
                Pair(seed, fingerprint)
            }) { result ->
                result.fold(
                    onSuccess = { (seed, fingerprint) ->
                        fingerprint?.let { app.backups.rememberFingerprint(name, it) }
                        then(seed, pass)
                    },
                    onFailure = { e ->
                        pass.wipe()
                        if (e is SeedLockException.WrongPassword) {
                            app.say("Wrong password", "That password does not open this backup. Try again (upper and lower case count).") {
                                unlock(app, name, text, withPassphrase, then)
                            }
                        } else {
                            app.say("Not opened", e.toUserText())
                        }
                    }
                )
            }
        }
        .setNegativeButton("Cancel") { _, _ ->
            password.text.clear()
            passphrase?.text?.clear()
        })
    app.promptsForPassword(dialog) {
        password.text.clear()
        passphrase?.text?.clear()
    }
}
