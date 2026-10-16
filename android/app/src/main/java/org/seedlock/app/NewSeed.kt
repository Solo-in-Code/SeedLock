package org.seedlock.app

import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.widget.EditText
import org.seedlock.ffi.BackupStrength
import org.seedlock.ffi.Seed
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.checkBackupName
import org.seedlock.ffi.countDiceRolls
import org.seedlock.ffi.diceRollsNeeded
import org.seedlock.ffi.generatePassword
import org.seedlock.ffi.minPasswordLength
import org.seedlock.ffi.wordSuggestions
import java.security.SecureRandom

/** Creating a new seed (the command-line program's menu 1): its words, a check, then its backup. */
class NewSeedScreen(app: MainActivity) : Screen(app) {
    override val title = "New seed"

    override fun build(): View {
        val page = Page(app)
        page.text("You are about to create a NEW seed. Do this in private: nobody looking, and no camera pointed at the screen.")
        val dice = page.checkBox("Add my own dice rolls as extra randomness")
        page.small("Your rolls are mixed with the phone's own randomness, so a few careless rolls cannot hurt.")
        page.heading("How many words?")
        page.button("24 words (recommended)") { start(24, dice.isChecked) }
        page.button("12 words") { start(12, dice.isChecked) }
        return page.view
    }

    private fun start(words: Int, dice: Boolean) {
        if (dice) {
            app.replace(DiceScreen(app, words))
            return
        }
        app.busy("Creating the seed...", { Seed.generate(words) }) { result ->
            result.fold(
                onSuccess = { seed -> app.replace(NewWordsScreen(app, seed)) },
                onFailure = { e -> app.say("Not created", e.toUserText()) }
            )
        }
    }
}

/** Dice rolls typed in, with a counter, for a new seed. */
class DiceScreen(app: MainActivity, private val words: Int) : Screen(app) {
    override val title = "Dice rolls"

    override fun build(): View {
        val page = Page(app)
        val need = try {
            diceRollsNeeded(words, false)
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
            return page.view
        }
        page.text("Roll a six-sided die $need times, and type each result (the digits 1 to 6).")
        val counter = page.text("0 of $need rolls")
        val field = page.field("Your rolls", FieldKind.DICE)
        field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                counter.text = try {
                    val n = countDiceRolls(s?.toString() ?: "")
                    if (n >= need) "$n of $need rolls: enough." else "$n of $need rolls"
                } catch (e: SeedLockException) {
                    e.toUserText()
                }
            }
        })
        page.button("Create the seed") {
            val rolls = field.text.toString()
            app.busy("Creating the seed...", { Seed.fromDice(rolls, words, false) }) { result ->
                result.fold(
                    onSuccess = { seed ->
                        field.text.clear()
                        app.replace(NewWordsScreen(app, seed))
                    },
                    onFailure = { e -> app.say("Not created", e.toUserText()) }
                )
            }
        }
        return page.view
    }
}

/** The new seed's words, to write on paper. */
class NewWordsScreen(app: MainActivity, private var seed: Seed?) : Screen(app) {
    override val title = "Your new seed"

    override fun build(): View {
        val page = Page(app)
        val s = seed ?: return page.view
        val words = try {
            s.words()
        } catch (e: SeedLockException) {
            page.warning(e.toUserText())
            return page.view
        }
        page.text("Your new seed: ${words.size} words. Write them on paper, in this exact order.")
        page.code(numbered(words), copyable = false)
        page.text("Wallet fingerprint: ${fingerprintOf(s)} (a short ID that lets you recognise this wallet later).")
        page.button("I wrote every word down") { app.replace(QuizScreen(app, handOver())) }
        return page.view
    }

    private fun handOver(): Seed? {
        val s = seed
        seed = null
        return s
    }

    override fun closed() {
        seed?.close()
        seed = null
    }
}

/** Three of the words typed back from the paper, to make sure they were copied correctly. */
class QuizScreen(app: MainActivity, private var seed: Seed?) : Screen(app) {
    override val title = "Quick check"
    private val picks: List<Int>

    init {
        val count = seed?.wordCount() ?: 12
        val random = SecureRandom()
        val chosen = LinkedHashSet<Int>()
        while (chosen.size < 3) chosen.add(random.nextInt(count))
        picks = chosen.sorted()
    }

    override fun build(): View {
        val page = Page(app)
        val s = seed ?: return page.view
        page.text("Type these three words from your paper, to make sure they were copied correctly.")
        val fields: List<EditText> = picks.map { p -> page.field("Word #${p + 1}", FieldKind.WORDS) }
        page.button("Check") {
            val words = s.words()
            val ok = picks.indices.all { i -> sameWord(fields[i].text.toString().trim().lowercase(), words[picks[i]]) }
            fields.forEach { it.text.clear() }
            if (ok) {
                app.replace(SaveSeedScreen(app, handOver()))
            } else {
                app.say("Not the same", "That did not match the words you were shown. Let's look at them again.") {
                    app.replace(NewWordsScreen(app, handOver()))
                }
            }
        }
        return page.view
    }

    /** The whole word, or its first letters when no other word starts with them. */
    private fun sameWord(typed: String, word: String): Boolean =
        typed == word || (typed.length >= 3 && word.startsWith(typed) && wordSuggestions(typed).size == 1)

    private fun handOver(): Seed? {
        val s = seed
        seed = null
        return s
    }

    override fun closed() {
        seed?.close()
        seed = null
    }
}

/** The words of a seed one already has, to encrypt into a backup (the command-line program's menu 2). */
class EnterWordsScreen(app: MainActivity) : Screen(app) {
    override val title = "Encrypt a seed"

    override fun build(): View {
        val page = Page(app)
        page.text("Type the words of your seed (12 to 24), separated by spaces. The first four letters of each word are enough.")
        page.small("Do this in private: nobody looking, and no camera pointed at the screen.")
        val field = page.field("Your words", FieldKind.WORDS)
        page.button("Next") {
            val words = field.text.toString()
            try {
                val seed = Seed.fromWords(words)
                field.text.clear()
                app.replace(SaveSeedScreen(app, seed))
            } catch (e: SeedLockException) {
                app.say("Check the words", e.toUserText())
            }
        }
        return page.view
    }
}

/**
 * Encrypts a seed into a backup with a password, saves it on this phone, then reads the saved file back
 * and opens it with the password: proof that what is stored opens (as the command-line program does).
 */
class SaveSeedScreen(app: MainActivity, private var seed: Seed?) : Screen(app) {
    override val title = "Save the backup"
    private var generated: String? = null

    override fun build(): View {
        val page = Page(app)
        val s = seed ?: return page.view
        page.text("Wallet fingerprint: ${fingerprintOf(s)}")
        page.text(
            "The seed is now encrypted with a password into a backup. Keep the password in your password manager " +
                "(on your everyday phone or computer): without it, the backup can never be opened."
        )
        page.heading("Name")
        val name = page.field("Name of the backup", FieldKind.NAME)
        page.heading("Password")
        val shown = generated
        val password: EditText
        val again: EditText
        if (shown != null) {
            page.text("Your new password (8 random words). Save it in your password manager now, exactly as shown:")
            page.code(shown, copyable = false)
            page.button("Copy it, for a password manager on this phone") {
                app.copySecret("SeedLock password", shown)
                app.say(
                    "Copied",
                    "Now open your password manager, paste the password into a new entry and save it, and copy it " +
                        "from there again. Then come back to SeedLock within ${MainActivity.KEEP_OPEN_MS / 60_000} " +
                        "minutes (this screen stays open meanwhile) and paste it here. When you come back, SeedLock " +
                        "empties the clipboard if its own copy of the password is still on it."
                )
            }
            page.small("A password manager on another phone or computer: type the password there by hand.")
            page.text("Then type it here, copied back from the password manager, to prove it was saved correctly:")
            password = page.field("The password", FieldKind.PASSWORD)
            again = password
        } else {
            page.button("Generate a strong password for me") {
                generated = try {
                    generatePassword()
                } catch (e: SeedLockException) {
                    null
                }
                app.refresh()
            }
            page.text("Or type your own (at least ${minPasswordLength()} characters):")
            password = page.field("Password", FieldKind.PASSWORD)
            again = page.field("The same password again", FieldKind.PASSWORD)
        }
        val light = page.checkBox("Old phone with little memory: protect it with 64 MiB instead of 256 MiB (it still opens everywhere)")
        page.button("Encrypt and save") { save(s, name, password, again, shown, light.isChecked) }
        return page.view
    }

    private fun save(s: Seed, nameField: EditText, password: EditText, again: EditText, shown: String?, light: Boolean) {
        val name = try {
            checkBackupName(nameField.text.toString())
        } catch (e: SeedLockException) {
            app.say("Name", e.toUserText())
            return
        }
        app.backups.taken(name)?.let {
            app.say("Name", "There is already a backup named \"$it\" on this phone. Please choose another name.")
            return
        }
        if (shown != null) {
            if (!sameText(password.text, shown)) {
                password.text.clear()
                app.say("Not the same", "That does not match the password shown. Check the password manager's entry and try again.")
                return
            }
        } else {
            if (password.text.length < minPasswordLength()) {
                app.say("Password", "Too short: use at least ${minPasswordLength()} characters.")
                return
            }
            if (!sameText(password.text, again.text)) {
                password.text.clear()
                again.text.clear()
                app.say("Password", "The two passwords are different. Please type them again.")
                return
            }
        }
        val secret = password.takeSecret()
        again.text.clear()
        val strength = if (light) BackupStrength.LIGHT else BackupStrength.STANDARD
        val mib = if (light) 64 else 256
        app.busy("Encrypting with Argon2id ($mib MiB), saving, and opening the saved backup again to check it...", {
            try {
                val backup = withCopy(secret) { s.encrypt(it, strength) }
                app.backups.save(name, backup.fileContents)
                withCopy(secret) { Seed.unlock(app.backups.read(name), it) }.use { reopened ->
                    if (!reopened.sameAs(s)) throw UserError("The saved backup does not open to the same seed. Do NOT rely on it; please try again.")
                }
                s.fingerprint()
            } finally {
                secret.wipe()
            }
        }) { result ->
            result.fold(
                onSuccess = { fingerprint ->
                    app.backups.rememberFingerprint(name, fingerprint)
                    generated = null
                    app.home()
                    app.open(BackupScreen(app, name))
                    app.say(
                        "Backup saved",
                        "The encrypted backup \"$name\" is saved on this phone and checked: it opens with the password.\n\n" +
                            "Next: keep a copy off this phone too (\"Save a copy as a file\" onto an SD card or a USB stick, " +
                            "or \"Show it as a QR code\"), and keep the password in your password manager, apart from the backup."
                    )
                },
                onFailure = { e -> app.say("Not saved", e.toUserText()) }
            )
        }
    }

    override fun closed() {
        seed?.close()
        seed = null
        generated = null
        app.endCopy()
    }
}
