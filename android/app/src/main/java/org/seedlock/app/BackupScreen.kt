package org.seedlock.app

import android.view.View
import org.seedlock.ffi.Seed
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.backupInfo
import org.seedlock.ffi.backupQrText
import org.seedlock.ffi.qrCode

/** One backup on this phone, and what can be done with it. */
class BackupScreen(app: MainActivity, private val name: String) : Screen(app) {
    // read each time the screen is shown: opening the backup may have made its fingerprint known
    override val title: String
        get() = app.backups.label(name)

    override fun build(): View {
        val page = Page(app)
        val text = try {
            app.backups.read(name)
        } catch (e: Exception) {
            page.warning("This backup's file cannot be read: ${e.toUserText()}")
            return page.view
        }
        val info = try {
            backupInfo(text)
        } catch (e: SeedLockException) {
            page.warning("This backup is damaged: ${e.toUserText()}")
            return page.view
        }
        page.text("Encrypted backup \"$name\". Opening it takes its password, ${info.memoryMib} MiB of memory and a few seconds.")
        val fingerprint = app.backups.fingerprint(name)
        if (fingerprint != null) {
            page.text("Wallet fingerprint: $fingerprint")
            page.small("Without a BIP39 passphrase. With one, the seed is another wallet, with another fingerprint (SeedLock keeps nothing about it).")
        } else {
            page.small("Its wallet fingerprint shows here once the backup was opened with its password (for example with \"Check the backup\").")
        }

        page.heading("Use it")
        page.button("Sign a transaction") { SignFlow(app, name, text).start() }
        page.button("Wallet for Sparrow (xpub, descriptor, addresses)") { sparrowFlow(app, name, text) }
        page.button("Key for a multisig wallet (as one of its cosigners)") { cosignerFlow(app, name, text) }

        page.heading("Check it")
        page.button("Check the backup (its password, and the words on paper)") {
            unlock(app, name, text, false) { seed, pass ->
                pass.wipe()
                app.open(CheckScreen(app, seed))
            }
        }
        page.button("Show the words") {
            app.confirm(
                "Show the words?",
                "Make sure that nobody can see this screen and that no camera points at it. (Screenshots are blocked in SeedLock.)",
                "Show them"
            ) {
                unlock(app, name, text, false) { seed, pass ->
                    pass.wipe()
                    app.open(WordsScreen(app, seed))
                }
            }
        }

        page.heading("Copy it")
        page.small(
            "It is encrypted: without its password it is useless, so a copy is safe on an SD card, a USB stick, " +
                "on paper or in a cloud folder."
        )
        page.button("Show it as a QR code") {
            val code = try {
                backupQrText(text, name)
            } catch (e: SeedLockException) {
                app.say("Damaged backup", e.toUserText())
                return@button
            }
            app.open(
                QrScreen(
                    app, "Backup \"$name\"", listOf(code), false,
                    "The backup's QR code, with its name: scan it with SeedLock on another phone or computer."
                ) { page ->
                    page.button("Save it as a picture (PNG)") { saveQrPicture(app, name, text) }
                    page.small(PICKER_HINT)
                }
            )
        }
        page.button("Save its QR code as a picture (PNG), to print or keep") { saveQrPicture(app, name, text) }
        page.button("Save a copy as a file (.seedlock)") {
            app.createFile("$name${Backups.EXT}", "application/octet-stream") { uri ->
                if (uri == null) return@createFile
                try {
                    writeFile(app, uri, text.toByteArray(Charsets.UTF_8))
                    app.say("Saved", "A copy of the backup \"$name\" is saved: ${describePlace(app, uri)}.")
                } catch (e: Exception) {
                    app.say("Not saved", e.toUserText())
                }
            }
        }
        page.small(PICKER_HINT)

        page.heading("Remove it")
        page.button("Delete it from this phone") {
            app.confirm(
                "Delete \"$name\"?",
                "The backup is deleted from this phone only. Do this only if you have other copies of it (or the words on paper).",
                "Delete"
            ) {
                app.backups.delete(name)
                app.home()
            }
        }
        return page.view
    }
}

/** The backup opened with its password: its fingerprint, and a comparison with the words on paper. */
class CheckScreen(app: MainActivity, private var seed: Seed?) : Screen(app) {
    override val title = "Check the backup"
    private var verdict: String? = null
    private var matched = false

    override fun build(): View {
        val page = Page(app)
        val s = seed ?: return page.view
        page.good("Backup OK: it opens with this password, and its seed is valid.")
        page.text("Wallet fingerprint (without a passphrase): ${fingerprintOf(s)}")
        page.heading("Compare it with the words on paper")
        page.small("Type the ${s.wordCount()} words of your paper backup, separated by spaces. The first four letters of each word are enough.")
        val words = page.field("Words", FieldKind.WORDS)
        verdict?.let { if (matched) page.good(it) else page.warning(it) }
        page.button("Compare") {
            val typed = words.text.toString()
            words.text.clear()
            try {
                matched = s.matchesWords(typed)
                verdict = if (matched) "MATCH: the words you typed are this backup's seed." else "MISMATCH: the words you typed are a different seed than this backup."
            } catch (e: SeedLockException) {
                matched = false
                verdict = "Not compared: ${e.toUserText()}"
            }
            app.refresh()
        }
        return page.view
    }

    override fun closed() {
        seed?.close()
        seed = null
    }
}

/** The words of an opened backup. */
class WordsScreen(app: MainActivity, private var seed: Seed?) : Screen(app) {
    override val title = "The words"

    override fun build(): View {
        val page = Page(app)
        val s = seed ?: return page.view
        page.text("The ${s.wordCount()} words of this seed, in order. Make sure nobody can see your screen.")
        page.code(numbered(s.words()), copyable = false)
        page.text("Wallet fingerprint: ${fingerprintOf(s)}")
        page.button("Hide the words") { app.back() }
        return page.view
    }

    override fun closed() {
        seed?.close()
        seed = null
    }
}

/** What to know about the place Android asks for when SeedLock saves a file. */
const val PICKER_HINT =
    "Android then asks where to save it. Its menu (☰) offers the other places: the phone's folders, an SD card, " +
        "a USB stick, and a cloud folder if the phone has one."

/**
 * Saves a backup's QR code (with its name: see backupQrText) as a PNG picture with what it is written
 * under it, to print or to keep; SeedLock on another phone or computer scans it from the picture.
 */
fun saveQrPicture(app: MainActivity, name: String, text: String) {
    val picture = try {
        qrPicture(qrCode(backupQrText(text, name)), listOf("SeedLock backup \"$name\"", "Encrypted: useless without its password"))
    } catch (e: SeedLockException) {
        app.say("Damaged backup", e.toUserText())
        return
    }
    app.createFile("$name QR code.png", "image/png") { uri ->
        if (uri == null) return@createFile
        try {
            writePng(app, uri, picture)
            app.say("Saved", "The backup's QR code is saved as a picture: ${describePlace(app, uri)}.")
        } catch (e: Exception) {
            app.say("Not saved", e.toUserText())
        }
    }
}

/** Words numbered one per line, as they are written on paper. */
fun numbered(words: List<String>): String =
    words.mapIndexed { i, w -> "${(i + 1).toString().padStart(2)}. $w" }.joinToString("\n")

fun fingerprintOf(seed: Seed): String = try {
    seed.fingerprint()
} catch (e: SeedLockException) {
    "(unknown: ${e.toUserText()})"
}
