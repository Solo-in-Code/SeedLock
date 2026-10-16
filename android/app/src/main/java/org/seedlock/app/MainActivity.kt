package org.seedlock.app

import android.Manifest
import android.app.Activity
import android.app.Dialog
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
import android.os.SystemClock
import android.view.View
import android.view.WindowManager
import java.io.File
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/** One page of the app. The activity shows the top one of a stack of screens. */
abstract class Screen(val app: MainActivity) {
    abstract val title: String

    /** Builds the page. Called each time the screen comes back to the top. */
    abstract fun build(): View

    /** The screen is now on top (start the camera, an animation...). */
    open fun shown() {}

    /** Another screen covers it, or it is closing (stop the camera, the animation...). */
    open fun hidden() {}

    /** The screen is gone for good: let go of what it holds (seeds, wallets). */
    open fun closed() {}

    /** The answer to a permission request made while this screen was on top. */
    open fun permissionResult(granted: Boolean) {}

    /**
     * The app has the screen's attention again (after the notification shade, a dialog, another app):
     * a screen that shows something that may have changed meanwhile looks again.
     */
    open fun focused() {}
}

/**
 * SeedLock's only activity. Seeds and wallets live in Rust (seedlock-ffi) and are let go of when the
 * screens that hold them close; every screen above the first one, and every dialog, closes as soon as
 * the app is left.
 */
class MainActivity : Activity() {
    private val stack = ArrayList<Screen>()

    /** The dialogs on the screen now (see [showSecure]). */
    private val dialogs = ArrayList<Dialog>()

    /** False from the moment the app is left (onStop) until it is on the screen again (onStart). */
    private var onScreen = false

    /** The background thread for slow work (opening and making backups, signing). */
    val worker: ExecutorService = Executors.newSingleThreadExecutor()

    lateinit var backups: Backups
        private set

    lateinit var multisig: MultisigWallets
        private set

    private var pendingFile: ((Uri?) -> Unit)? = null

    /** True while a file picker that the app opened is on the screen: the app has not been left. */
    private var inPicker = false

    /** True while Android asks the person for the camera on the app's behalf: the app has not been left. */
    private var askingPermission = false

    /** Where a file was last opened or saved: the file pickers start there the next time. */
    private var lastFile: Uri? = null

    /** The label of a secret that [copySecret] put on the clipboard, until SeedLock empties it again. */
    private var copiedLabel: String? = null

    /**
     * Until when (SystemClock.elapsedRealtime) SeedLock may be left without closing what is open: for a
     * few minutes after [copySecret], to paste the password into a password manager on this phone.
     */
    private var keepOpenUntil = 0L

    /** True when SeedLock was left and what was open stayed open (see [keepOpenUntil]). */
    private var keptOpen = false

    /** True once SeedLock was left after [copySecret]: back in front, it empties the clipboard. */
    private var leftWithCopy = false

    /**
     * The answer of a file picker (or of the camera question) that arrived before SeedLock was on the
     * screen again. Up to Android 12, Android hands an activity the result of another one before its
     * onStart (Android 10's ActivityResultItem has no lifecycle state of its own): until onStart, SeedLock
     * still counts as left and would show no dialog and no screen, so the answer waits for onStart.
     */
    private var waitingAnswer: (() -> Unit)? = null

    /**
     * True while the app is left (another app, the home screen, the screen switched off). Work that ends
     * meanwhile, such as opening a backup, shows nothing then: a screen it opens closes at once, letting go
     * of what it holds, and no dialog appears.
     */
    val left: Boolean
        get() = !onScreen && !inPicker && !askingPermission

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // No screenshots, no screen recording, and no picture of the app in the list of recent apps.
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        noAutofill(window.decorView)
        backups = Backups(File(filesDir, "backups"))
        multisig = MultisigWallets(File(filesDir, "multisig"))
        stack.add(HomeScreen(this))
        render()
    }

    /** Shows a new screen on top. */
    fun open(screen: Screen) {
        if (left) {
            screen.closed()
            return
        }
        stack.lastOrNull()?.hidden()
        stack.add(screen)
        render()
    }

    /** Shows a new screen in place of the top one, which closes. */
    fun replace(screen: Screen) {
        if (left) {
            screen.closed()
            return
        }
        if (stack.size > 1) {
            val top = stack.removeAt(stack.size - 1)
            top.hidden()
            top.closed()
        }
        stack.add(screen)
        render()
    }

    /** Closes the top screen. */
    fun back() {
        if (stack.size <= 1) {
            finish()
            return
        }
        val top = stack.removeAt(stack.size - 1)
        top.hidden()
        top.closed()
        render()
    }

    /** Back to the first screen: every other one closes, and lets go of what it holds. */
    fun home() {
        stack.lastOrNull()?.hidden()
        while (stack.size > 1) {
            stack.removeAt(stack.size - 1).closed()
        }
        render()
    }

    /** Builds the top screen again (after something on it changed). */
    fun refresh() {
        render()
    }

    private fun render() {
        val top = stack.lastOrNull() ?: return
        title = top.title
        setContentView(top.build())
        top.shown()
    }

    @Deprecated("Android's own back handling; kept for every Android version this app supports")
    override fun onBackPressed() {
        back()
    }

    /** Keeps track of a dialog while it is on the screen, so that leaving the app closes it. */
    fun track(dialog: Dialog) {
        dialogs.add(dialog)
        dialog.setOnDismissListener { dialogs.remove(dialog) }
    }

    override fun onStart() {
        super.onStart()
        onScreen = true
        waitingAnswer?.let {
            waitingAnswer = null
            it()
        }
        if (keptOpen) {
            keptOpen = false
            if (SystemClock.elapsedRealtime() > keepOpenUntil && stack.size > 1) {
                home()
                say(
                    "Closed",
                    "You were away from SeedLock for longer than ${KEEP_OPEN_MS / 60_000} minutes after copying the " +
                        "password, so it closed what was open, as it does whenever it is left. A new seed's words are " +
                        "on your paper: \"Encrypt the words of a seed I already have\" makes its backup."
                )
            }
        }
    }

    override fun onStop() {
        super.onStop()
        onScreen = false
        if (copiedLabel != null) leftWithCopy = true
        if (inPicker || askingPermission || isChangingConfigurations) return
        for (dialog in ArrayList(dialogs)) dialog.dismiss()
        if (SystemClock.elapsedRealtime() <= keepOpenUntil) {
            // gone to paste a copied password into a password manager on this phone: back soon
            keptOpen = true
            return
        }
        // Leaving the app (another app, the home screen, the screen switched off) forgets every open seed.
        if (stack.size > 1) home()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) return
        if (leftWithCopy) {
            leftWithCopy = false
            emptyClipboard()
        }
        stack.lastOrNull()?.focused()
    }

    /**
     * Puts a secret (a new password) on the clipboard, to paste it into a password manager on this phone.
     * For [KEEP_OPEN_MS] from now, SeedLock may be left for that without closing what is open (the screen
     * that copied it ends this with [endCopy]). Once SeedLock is in front again after it was left, it
     * empties the clipboard if the secret is still on it; something copied since then stays (the password
     * copied back from the password manager, for example).
     */
    fun copySecret(label: String, text: String) {
        val clip = ClipData.newPlainText(label, text)
        if (Build.VERSION.SDK_INT >= 24) {
            // ClipDescription.EXTRA_IS_SENSITIVE: Android 13 and newer then show no preview of it
            clip.description.extras = PersistableBundle().apply { putBoolean("android.content.extra.IS_SENSITIVE", true) }
        }
        clipboard().setPrimaryClip(clip)
        copiedLabel = label
        keepOpenUntil = SystemClock.elapsedRealtime() + KEEP_OPEN_MS
    }

    /** The copied secret's screen is closing: leaving SeedLock closes everything again, and the clipboard is emptied. */
    fun endCopy() {
        keepOpenUntil = 0L
        if (hasWindowFocus()) emptyClipboard() else leftWithCopy = true
    }

    /** Empties the clipboard if it still holds what [copySecret] put there (it can be read while SeedLock has the focus). */
    private fun emptyClipboard() {
        val label = copiedLabel ?: return
        val manager = clipboard()
        val stillOurs = try {
            manager.primaryClipDescription?.label?.toString() == label
        } catch (e: Exception) {
            false
        }
        if (stillOurs) {
            if (Build.VERSION.SDK_INT >= 28) manager.clearPrimaryClip() else manager.setPrimaryClip(ClipData.newPlainText("", ""))
        }
        copiedLabel = null
    }

    private fun clipboard(): ClipboardManager = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    override fun onDestroy() {
        for (dialog in ArrayList(dialogs)) dialog.dismiss()
        stack.lastOrNull()?.hidden()
        while (stack.isNotEmpty()) {
            stack.removeAt(stack.size - 1).closed()
        }
        worker.shutdown()
        super.onDestroy()
    }

    /** Asks the person to let SeedLock use the camera; the answer goes to the screen on top. */
    fun askForCamera() {
        askingPermission = true
        requestPermissions(arrayOf(Manifest.permission.CAMERA), REQUEST_CAMERA)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        val granted = grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED
        whenOnScreen {
            askingPermission = false
            stack.lastOrNull()?.permissionResult(granted)
        }
    }

    /** Runs `answer` now if SeedLock is on the screen, else as soon as it is (see [waitingAnswer]). */
    private fun whenOnScreen(answer: () -> Unit) {
        if (onScreen) answer() else waitingAnswer = answer
    }

    /** Lets the person choose a file (on the phone, an SD card or a USB stick); gives its address, or null. */
    fun pickFile(then: (Uri?) -> Unit) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        startPicker(intent, then)
    }

    /**
     * Lets the person choose where to save a new file named `name` (of the kind `mime`): a folder on the
     * phone, an SD card or a USB stick, or a cloud folder if the phone has one. Gives its address, or null.
     */
    fun createFile(name: String, mime: String, then: (Uri?) -> Unit) {
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType(mime)
            .putExtra(Intent.EXTRA_TITLE, name)
        startPicker(intent, then)
    }

    private fun startPicker(intent: Intent, then: (Uri?) -> Unit) {
        val last = lastFile
        if (last != null && Build.VERSION.SDK_INT >= 26) {
            // DocumentsContract.EXTRA_INITIAL_URI: start in the folder of the file last opened or saved
            intent.putExtra("android.provider.extra.INITIAL_URI", last)
        }
        pendingFile = then
        inPicker = true
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, REQUEST_FILE)
        } catch (e: ActivityNotFoundException) {
            inPicker = false
            pendingFile = null
            say("No file manager", "This phone has no app for choosing files.")
        }
    }

    @Deprecated("Android's own result handling; kept for every Android version this app supports")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_FILE) return
        val then = pendingFile
        pendingFile = null
        val uri = if (resultCode == RESULT_OK) data?.data else null
        if (uri != null) lastFile = uri
        // Until then the picker still counts as open: SeedLock was not left.
        whenOnScreen {
            inPicker = false
            then?.invoke(uri)
        }
    }

    companion object {
        const val REQUEST_FILE = 1
        const val REQUEST_CAMERA = 2

        /** For this long after [copySecret], SeedLock may be left without closing what is open. */
        const val KEEP_OPEN_MS = 5 * 60_000L
    }
}
