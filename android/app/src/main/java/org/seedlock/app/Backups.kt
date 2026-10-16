package org.seedlock.app

import android.app.Activity
import android.content.Context
import android.graphics.Bitmap
import android.net.ConnectivityManager
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.Settings
import org.seedlock.ffi.MultisigWallet
import org.seedlock.ffi.RegisteredMultisig
import org.seedlock.ffi.backupText
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream

/**
 * Files of one kind kept in the app's own storage, which other apps cannot read: one file per name,
 * `<name><ext>`, the same files as the command-line program keeps. `what` says what they hold ("backup").
 */
open class NamedFiles(protected val dir: File, private val ext: String, val what: String) {
    /** The names, in alphabetical order. */
    fun names(): List<String> =
        (dir.listFiles() ?: emptyArray())
            .filter { it.isFile && it.name.endsWith(ext) }
            .map { it.name.removeSuffix(ext) }
            .sortedWith(String.CASE_INSENSITIVE_ORDER)

    fun read(name: String): String = file(name).readText()

    /**
     * The file that already uses this name, ignoring upper and lower case (computers, USB sticks and
     * cloud folders treat "Wallet" and "wallet" as one file).
     */
    fun taken(name: String): String? = names().firstOrNull { it.equals(name, ignoreCase = true) }

    /** Saves a new file. Never replaces one: the name must be free. */
    open fun save(name: String, contents: String) {
        taken(name)?.let { throw UserError("There is already a $what named \"$it\" on this phone.") }
        if (!dir.isDirectory && !dir.mkdirs()) throw IOException("cannot make the folder for the ${what}s")
        val temp = File(dir, "$name$ext.part")
        FileOutputStream(temp).use { out ->
            out.write(contents.toByteArray(Charsets.UTF_8))
            out.fd.sync()
        }
        if (!temp.renameTo(file(name))) {
            temp.delete()
            throw IOException("cannot save the $what")
        }
    }

    open fun delete(name: String): Boolean = file(name).delete()

    private fun file(name: String) = File(dir, "$name$ext")
}

/**
 * The backups kept on this phone: encrypted `.seedlock` files (the same as the command-line program
 * writes) in the app's own storage. Next to each, once the backup was opened on this phone (or made here),
 * its wallet's fingerprint without a BIP39 passphrase (`<name>.fingerprint`), to show it beside the
 * backup's name and to tell which backups are the cosigners of a multisig wallet. Nothing about a
 * passphrase is ever kept: with one, a seed is another wallet, whose existence stays the person's secret.
 */
class Backups(dir: File) : NamedFiles(dir, EXT, "backup") {
    /** The wallet fingerprint of the backup `name` (without a passphrase), once it is known on this phone. */
    fun fingerprint(name: String): String? = try {
        File(dir, "$name$FINGERPRINT").readText().trim().takeIf { FINGERPRINT_FORM.matches(it) }
    } catch (e: Exception) {
        null
    }

    /** Remembers the wallet fingerprint (without a passphrase) of the backup `name`. */
    fun rememberFingerprint(name: String, fingerprint: String) {
        if (!FINGERPRINT_FORM.matches(fingerprint) || fingerprint(name) == fingerprint || !File(dir, "$name$EXT").isFile) return
        try {
            File(dir, "$name$FINGERPRINT").writeText(fingerprint)
        } catch (e: Exception) {
            // only a convenience: without it, the backup simply shows no fingerprint
        }
    }

    /** The names of the backups on this phone whose wallet has this fingerprint (without a passphrase). */
    fun withFingerprint(fingerprint: String): List<String> = names().filter { fingerprint(it) == fingerprint }

    /** The backup's name, with its wallet fingerprint beside it when it is known: "Savings (73c5da0a)". */
    fun label(name: String): String = fingerprint(name)?.let { "$name ($it)" } ?: name

    override fun save(name: String, contents: String) {
        super.save(name, contents)
        File(dir, "$name$FINGERPRINT").delete() // a new backup: no fingerprint known yet
    }

    override fun delete(name: String): Boolean {
        File(dir, "$name$FINGERPRINT").delete()
        return super.delete(name)
    }

    /** The backup on this phone that is this very backup (the same backup line), if there is one. */
    fun same(text: String): String? {
        val wanted = try {
            backupText(text)
        } catch (e: Exception) {
            return null
        }
        return names().firstOrNull { name ->
            try {
                backupText(read(name)) == wanted
            } catch (e: Exception) {
                false
            }
        }
    }

    companion object {
        const val EXT = ".seedlock"
        private const val FINGERPRINT = ".fingerprint"
        private val FINGERPRINT_FORM = Regex("[0-9a-f]{8}")
    }
}

/**
 * The multisig wallets kept on this phone: `.multisig` files, the same as the command-line program keeps
 * in its "multisig wallets" folder (the wallet's output descriptor, under a few comment lines). They hold
 * no secrets, only the wallets' public keys.
 */
class MultisigWallets(dir: File) : NamedFiles(dir, EXT, "multisig wallet") {
    /** The wallet kept under `name`; the error says why its file cannot be read. */
    fun wallet(name: String): MultisigWallet = MultisigWallet.fromText(read(name))

    /** Every multisig wallet on this phone that can be read: what checking and signing a transaction need. */
    fun all(): List<RegisteredMultisig> = names().mapNotNull { name ->
        try {
            RegisteredMultisig(name, wallet(name))
        } catch (e: Exception) {
            null
        }
    }

    /** The wallet on this phone that is this very wallet (the same kind, threshold and keys), if there is one. */
    fun same(wallet: MultisigWallet): String? = all().firstOrNull { it.wallet.sameAs(wallet) }?.name

    companion object {
        const val EXT = ".multisig"
    }
}

/** The name of a file that a file picker gave, if it says. */
fun displayName(activity: Activity, uri: Uri): String? = try {
    activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) cursor.getString(0) else null
    }
} catch (e: Exception) {
    null
}

/** The contents of a file that a file picker gave; `limit` bytes at most. */
fun readFile(activity: Activity, uri: Uri, limit: Int, what: String): ByteArray {
    val input = activity.contentResolver.openInputStream(uri) ?: throw IOException("the file cannot be opened")
    return input.use { readAtMost(it, limit, what) }
}

private fun readAtMost(input: InputStream, limit: Int, what: String): ByteArray {
    val out = ByteArrayOutputStream()
    val buffer = ByteArray(16 * 1024)
    while (true) {
        val n = input.read(buffer)
        if (n < 0) break
        out.write(buffer, 0, n)
        if (out.size() > limit) throw UserError("That file is too large to be $what.")
    }
    return out.toByteArray()
}

/** Writes a file to the address a file picker gave. */
fun writeFile(activity: Activity, uri: Uri, bytes: ByteArray) {
    val output = activity.contentResolver.openOutputStream(uri) ?: throw IOException("the file cannot be written")
    output.use { it.write(bytes) }
}

/** Writes a picture as a PNG file to the address a file picker gave. */
fun writePng(activity: Activity, uri: Uri, picture: Bitmap) {
    val output = activity.contentResolver.openOutputStream(uri) ?: throw IOException("the file cannot be written")
    output.use {
        if (!picture.compress(Bitmap.CompressFormat.PNG, 100, it)) throw IOException("the picture cannot be written")
    }
}

/**
 * Where a file that a file picker saved is, in words: its name and its place ("on this phone, in the
 * folder Download"), as far as the place it was saved to says.
 */
fun describePlace(activity: Activity, uri: Uri): String {
    val where = try {
        val id = DocumentsContract.getDocumentId(uri)
        when (uri.authority) {
            // the phone's own storage ("primary:Download/x.seedlock"), an SD card or a USB stick ("1A2B-3C4D:...")
            "com.android.externalstorage.documents" -> {
                val folder = id.substringAfter(':').substringBeforeLast('/', "")
                val inFolder = if (folder.isEmpty()) "" else ", in the folder \"$folder\""
                if (id.substringBefore(':') == "primary") "on this phone$inFolder" else "on the SD card or USB stick$inFolder"
            }
            // the Download folder ("raw:/storage/emulated/0/Download/x.seedlock", or a number)
            "com.android.providers.downloads.documents" ->
                if (id.startsWith("raw:")) "on this phone, in ${id.removePrefix("raw:").substringBeforeLast('/')}"
                else "on this phone, in the folder \"Download\""
            else -> {
                val label = activity.packageManager.resolveContentProvider(uri.authority ?: "", 0)
                    ?.loadLabel(activity.packageManager)?.toString()
                if (label.isNullOrBlank()) "where you chose" else "in \"$label\""
            }
        }
    } catch (e: Exception) {
        "where you chose"
    }
    val name = displayName(activity, uri)
    return if (name != null) "\"$name\", $where" else where
}

/** Why this phone may be online: airplane mode is off, or a network is connected. Empty when it looks offline. */
fun onlineReasons(context: Context): List<String> {
    val reasons = ArrayList<String>()
    val airplane = try {
        Settings.Global.getInt(context.contentResolver, Settings.Global.AIRPLANE_MODE_ON, 0) != 0
    } catch (e: Exception) {
        false
    }
    if (!airplane) reasons.add("Airplane mode is off.")
    try {
        val manager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        @Suppress("DEPRECATION")
        val active = manager.activeNetworkInfo
        @Suppress("DEPRECATION")
        if (active != null && active.isConnected) reasons.add("It is connected to a network (${active.typeName}).")
    } catch (e: Exception) {
        // no permission to look: nothing to say
    }
    return reasons
}
