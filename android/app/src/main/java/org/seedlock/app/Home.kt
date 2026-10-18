package org.seedlock.app

import android.app.AlertDialog
import android.view.View
import org.seedlock.ffi.ScanTarget
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.backupFileContents
import org.seedlock.ffi.backupInfo
import org.seedlock.ffi.backupNameIn
import org.seedlock.ffi.checkBackupName
import org.seedlock.ffi.seedlockVersion

/**
 * The first screen: the backups on this phone, and what can be done. While the phone is online, it says only
 * that SeedLock works only offline, and why the phone may be online (see [MainActivity.checkNetwork]).
 */
class HomeScreen(app: MainActivity) : Screen(app) {
    override val title = "SeedLock"

    override fun build(): View {
        val page = Page(app)
        val online = app.onlineNow
        if (online.isNotEmpty()) {
            page.warning(
                "SEEDLOCK WORKS ONLY OFFLINE. ${online.joinToString(" ")} Turn on airplane mode, and keep Wi-Fi, " +
                    "mobile data and Bluetooth off. SeedLock checks every second, and opens as soon as the phone is offline."
            )
            page.small(
                "SeedLock cannot use the internet in any case: Android does not give it the permission to. It still " +
                    "does nothing while the phone is online, because other apps could then send away what is on the " +
                    "screen or typed; and if the phone goes online while SeedLock is open, it closes everything at once."
            )
            page.button("Check again") { app.checkNow() }
            page.small("SeedLock ${seedlockVersion()}, phone app preview.")
            return page.view
        }
        page.good("This phone is offline: airplane mode is on, and no network is connected.")
        page.small("SeedLock cannot use the internet in any case: Android does not give it the permission to.")

        page.heading("Backups on this phone")
        val names = app.backups.names()
        if (names.isEmpty()) page.text("None yet. Add the backup you made on your computer, or create a new seed.")
        for (name in names) page.button(app.backups.label(name)) { app.open(BackupScreen(app, name)) }
        if (names.any { app.backups.fingerprint(it) == null }) {
            page.small("A backup's wallet fingerprint shows beside its name once the backup was opened on this phone.")
        }

        page.heading("Add a backup")
        page.button("Scan its QR code") { addByScan(app) }
        page.button("Open its file (.seedlock)") { addFromFile(app) }
        page.button("Paste its text") { app.open(PasteBackupScreen(app)) }

        page.heading("New")
        page.button("Create a new seed and its backup") { app.open(NewSeedScreen(app)) }
        page.button("Encrypt the words of a seed I already have") { app.open(EnterWordsScreen(app)) }

        page.heading("Multisig wallets")
        val wallets = app.multisig.names()
        if (wallets.isEmpty()) {
            page.small(
                "None yet. A multisig wallet needs several seeds to spend (for example any 2 of 3): Sparrow keeps the " +
                    "wallet, and each seed signs on its own. To make one, open a backup and choose \"Key for a multisig " +
                    "wallet\"; once the wallet is made in Sparrow, add it here."
            )
        }
        for (wallet in wallets) page.button(wallet) { app.open(MultisigScreen(app, wallet)) }
        page.button("Add a multisig wallet (from Sparrow)") { addMultisig(app) }

        page.small(
            "SeedLock ${seedlockVersion()}, phone app preview. It is not independently audited: try it with a " +
                "test wallet or a small amount first."
        )
        page.button("About SeedLock and its licenses") { app.open(AboutScreen(app)) }
        return page.view
    }
}

/** Adds a backup read from a QR code, a file or pasted text, under a name the person chooses. */
fun addBackup(app: MainActivity, text: String, suggestedName: String?) {
    val info = try {
        backupInfo(text)
    } catch (e: SeedLockException) {
        app.say("Not a backup", e.toUserText())
        return
    }
    val same = app.backups.same(text)
    if (same != null) {
        app.say("Already on this phone", "This backup is already on this phone, as \"$same\".") {
            app.home()
            app.open(BackupScreen(app, same))
        }
        return
    }
    askName(app, "The backup's name on this phone (it also names its file when you save a copy):", suggestedName ?: "", ::checkBackupName, app.backups) { name ->
        try {
            app.backups.save(name, backupFileContents(text))
            app.home()
            app.open(BackupScreen(app, name))
            app.say(
                "Backup added",
                "The backup \"$name\" is on this phone now. It opens with its password (that takes " +
                    "${info.memoryMib} MiB of memory and a few seconds). Its wallet fingerprint shows beside its " +
                    "name once it was opened here: the backup is encrypted, so only its password tells which wallet it is."
            )
        } catch (e: Exception) {
            app.say("Not saved", e.toUserText())
        }
    }
}

/**
 * Asks for a name (`prompt` says what for) until `check` takes it (it cleans the name up, or says what is
 * wrong with it) and `files` has nothing under that name yet.
 */
fun askName(app: MainActivity, prompt: String, suggested: String, check: (String) -> String, files: NamedFiles, then: (String) -> Unit) {
    val page = Page(app)
    page.text(prompt)
    val field = page.field("Name", FieldKind.NAME)
    field.setText(suggested)
    app.showSecure(AlertDialog.Builder(app)
        .setTitle("Name")
        .setView(page.view)
        .setPositiveButton("OK") { _, _ ->
            val typed = field.text.toString()
            val name = try {
                check(typed)
            } catch (e: SeedLockException) {
                app.say("Name", e.toUserText()) { askName(app, prompt, typed, check, files, then) }
                return@setPositiveButton
            }
            val taken = files.taken(name)
            if (taken != null) {
                app.say("Name", "There is already a ${files.what} named \"$taken\" on this phone. Please choose another name.") {
                    askName(app, prompt, name, check, files, then)
                }
                return@setPositiveButton
            }
            then(name)
        }
        .setNegativeButton("Cancel", null))
}

private fun addByScan(app: MainActivity) {
    app.open(ScanScreen(app, ScanTarget.BACKUP) { scanner ->
        val text = scanner.backupText()
        scanner.close()
        app.back()
        // the backup's QR codes that SeedLock's phone app shows carry the backup's name
        if (text != null) addBackup(app, text, backupNameIn(text))
    })
}

private fun addFromFile(app: MainActivity) {
    app.pickFile { uri ->
        if (uri == null) return@pickFile
        val text = try {
            String(readFile(app, uri, 64 * 1024, "a SeedLock backup"), Charsets.UTF_8)
        } catch (e: Exception) {
            app.say("Not read", e.toUserText())
            return@pickFile
        }
        addBackup(app, text, displayName(app, uri)?.removeSuffix(Backups.EXT))
    }
}

/** A backup typed or pasted in. */
class PasteBackupScreen(app: MainActivity) : Screen(app) {
    override val title = "Paste a backup"

    override fun build(): View {
        val page = Page(app)
        page.text("Paste the backup's text: the line that starts with SEEDLOCK1: (or the whole contents of its file).")
        val field = page.field("SEEDLOCK1:...", FieldKind.TEXT)
        page.button("Add this backup") {
            val text = field.text.toString()
            if (text.isNotBlank()) addBackup(app, text, backupNameIn(text))
        }
        return page.view
    }
}
