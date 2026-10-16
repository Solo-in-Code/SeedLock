package org.seedlock.app

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Build
import android.text.Editable
import android.text.InputType
import android.util.TypedValue
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import org.seedlock.ffi.QrImage
import org.seedlock.ffi.SeedLockException
import java.io.IOException
import java.nio.CharBuffer
import java.util.Arrays

const val RED = 0xFFB00020.toInt()
const val GREEN = 0xFF1B5E20.toInt()
const val BLUE = 0xFF0D47A1.toInt()
const val GREY = 0xFF555555.toInt()

fun Context.dp(value: Int): Int =
    TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value.toFloat(), resources.displayMetrics).toInt()

/** What a field is for: it decides the keyboard, and whether the text shows. */
enum class FieldKind { PASSWORD, WORDS, NAME, DICE, TEXT }

/** A page: a column that scrolls, with helpers that add the usual parts. */
class Page(val context: Context) {
    val column: LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        val p = context.dp(16)
        setPadding(p, p, p, p)
    }

    val view: ScrollView = ScrollView(context).apply {
        isFillViewport = true
        addView(column, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
    }

    fun <T : View> add(view: T, topDp: Int = 4): T {
        val params = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
        params.topMargin = context.dp(topDp)
        column.addView(view, params)
        return view
    }

    fun heading(text: String): TextView = add(TextView(context).apply {
        this.text = text
        textSize = 20f
        typeface = Typeface.DEFAULT_BOLD
    }, 16)

    fun text(text: String): TextView = add(TextView(context).apply {
        this.text = text
        textSize = 16f
    })

    fun small(text: String): TextView = add(TextView(context).apply {
        this.text = text
        textSize = 14f
        setTextColor(GREY)
    })

    /**
     * Text that must be copied exactly (an xpub, an address, a descriptor): fixed-width and selectable.
     * Secrets (a seed's words, a new password) are not selectable (`copyable = false`), so that they can
     * never reach the clipboard, where other apps and keyboards could read them.
     */
    fun code(text: String, copyable: Boolean = true): TextView = add(TextView(context).apply {
        this.text = text
        textSize = 15f
        typeface = Typeface.MONOSPACE
        setTextIsSelectable(copyable)
    })

    fun warning(text: String): TextView = add(banner(text, RED), 8)

    fun good(text: String): TextView = add(banner(text, GREEN), 8)

    /** Something important to know, neither good nor bad (where a multisig payment stands, for example). */
    fun note(text: String): TextView = add(banner(text, BLUE), 8)

    private fun banner(text: String, color: Int) = TextView(context).apply {
        this.text = text
        textSize = 16f
        setTextColor(Color.WHITE)
        setBackgroundColor(color)
        val p = context.dp(10)
        setPadding(p, p, p, p)
    }

    fun button(text: String, onClick: () -> Unit): Button = add(Button(context).apply {
        this.text = text
        setAllCaps(false)
        setOnClickListener { onClick() }
    }, 8)

    fun checkBox(text: String): CheckBox = add(CheckBox(context).apply {
        this.text = text
        textSize = 16f
    }, 8)

    fun field(hint: String, kind: FieldKind): EditText = add(secretField(context, hint, kind))

    /** A QR code, as large as the screen's width allows. */
    fun qr(code: QrImage): ImageView = add(qrView(context, code), 8)
}

/** EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING (Android 8 and newer; older keyboards ignore it). */
private const val IME_FLAG_NO_PERSONALIZED_LEARNING = 0x1000000

/** View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS (Android 8 and newer). */
private const val IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS = 0x8

/** A text field that keeps what is typed out of the keyboard's suggestions and memory, and out of autofill. */
fun secretField(context: Context, hint: String, kind: FieldKind): EditText = EditText(context).apply {
    this.hint = hint
    inputType = when (kind) {
        FieldKind.PASSWORD -> InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        FieldKind.WORDS -> InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        FieldKind.NAME -> InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        FieldKind.DICE -> InputType.TYPE_CLASS_NUMBER
        FieldKind.TEXT -> InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
    }
    // keyboards such as Gboard: do not learn or remember what is typed here (their "incognito" mode)
    imeOptions = imeOptions or IME_FLAG_NO_PERSONALIZED_LEARNING
    noAutofill(this)
}

/** Keeps autofill services (password managers and the like) away from a view and everything in it. */
fun noAutofill(view: View) {
    if (Build.VERSION.SDK_INT >= 26) {
        try {
            View::class.java.getMethod("setImportantForAutofill", Int::class.javaPrimitiveType)
                .invoke(view, IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS)
        } catch (e: Exception) {
            // an Android without autofill
        }
    }
}

/**
 * What was typed in a field, as UTF-8 bytes, without making a String of it (a String cannot be wiped);
 * the field is emptied. Whoever receives the bytes wipes them ([wipe]) once they are used.
 */
fun EditText.takeSecret(): ByteArray {
    val typed: Editable = text
    val chars = CharArray(typed.length)
    typed.getChars(0, typed.length, chars, 0)
    val encoded = Charsets.UTF_8.newEncoder().encode(CharBuffer.wrap(chars))
    val bytes = ByteArray(encoded.remaining())
    encoded.get(bytes)
    Arrays.fill(chars, '\u0000')
    if (encoded.hasArray()) Arrays.fill(encoded.array(), 0.toByte())
    typed.clear()
    return bytes
}

/** Overwrites the bytes of a password or passphrase. */
fun ByteArray.wipe() {
    Arrays.fill(this, 0.toByte())
}

/** Calls `use` with a copy of a secret, and wipes the copy afterwards (the copy goes to Rust, which wipes its own). */
fun <T> withCopy(secret: ByteArray, use: (ByteArray) -> T): T {
    val copy = secret.copyOf()
    try {
        return use(copy)
    } finally {
        copy.wipe()
    }
}

/** Whether two texts (typed in fields) are the same, compared without making Strings of them. */
fun sameText(a: CharSequence, b: CharSequence): Boolean {
    if (a.length != b.length) return false
    for (i in 0 until a.length) {
        if (a[i] != b[i]) return false
    }
    return true
}

/** A problem to tell the person about, in words. */
class UserError(message: String) : Exception(message)

/** What to tell the person about something that went wrong. */
fun Throwable.toUserText(): String = when (this) {
    is SeedLockException -> message ?: "Something went wrong."
    is UserError -> message ?: "Something went wrong."
    is IOException -> "The file could not be read or written (${message ?: javaClass.simpleName})."
    is OutOfMemoryError -> "This phone does not have enough free memory for this. Close other apps and try again."
    else -> "Something went wrong: $this"
}

/**
 * Shows a dialog. Like the screens, it cannot be captured in a screenshot or a screen recording (a
 * dialog is a window of its own, which the screens' FLAG_SECURE does not cover), and it closes when the
 * app is left; while the app is left, no dialog appears at all.
 */
fun Activity.showSecure(builder: AlertDialog.Builder): AlertDialog {
    val dialog = builder.create()
    dialog.window?.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
    if (this is MainActivity) {
        if (left) return dialog
        track(dialog)
    }
    dialog.show()
    return dialog
}

fun Activity.say(title: String, message: String, then: (() -> Unit)? = null) {
    showSecure(
        AlertDialog.Builder(this)
            .setTitle(title)
            .setMessage(message)
            .setCancelable(false)
            .setPositiveButton("OK") { _, _ -> then?.invoke() }
    )
}

fun Activity.confirm(title: String, message: String, yes: String, onYes: () -> Unit) {
    showSecure(
        AlertDialog.Builder(this)
            .setTitle(title)
            .setMessage(message)
            .setPositiveButton(yes) { _, _ -> onYes() }
            .setNegativeButton("Cancel", null)
    )
}

/** Asks to choose one of a few options (a button each), with an explanation above them. */
fun Activity.choose(title: String, message: String, options: List<String>, onChoice: (Int) -> Unit) {
    val page = Page(this)
    page.text(message)
    var dialog: AlertDialog? = null
    options.forEachIndexed { i, label ->
        page.button(label) {
            dialog?.dismiss()
            onChoice(i)
        }
    }
    dialog = showSecure(AlertDialog.Builder(this).setTitle(title).setView(page.view).setNegativeButton("Cancel", null))
}

/**
 * Runs `work` on the background thread while a message and a spinner show, then `done` on the
 * screen's thread with the result (or what went wrong).
 */
fun <T> MainActivity.busy(message: String, work: () -> T, done: (Result<T>) -> Unit) {
    val page = Page(this)
    page.add(ProgressBar(this), 8)
    page.text(message)
    val dialog = showSecure(AlertDialog.Builder(this).setView(page.view).setCancelable(false))
    worker.execute {
        val result = runCatching { work() }
        runOnUiThread {
            dialog.dismiss()
            if (!isFinishing && !isDestroyed) done(result)
        }
    }
}

/** Full brightness and the screen kept on while a QR code is shown, for the other camera to read it. */
fun Activity.qrShowing(on: Boolean) {
    if (on) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    val attributes = window.attributes
    attributes.screenBrightness = if (on) 1f else WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
    window.attributes = attributes
}

/** How many screen pixels each module of a QR code gets, so that the code (with its border) fits the screen's width. */
fun qrScale(context: Context, code: QrImage): Int {
    val metrics = context.resources.displayMetrics
    val available = minOf(metrics.widthPixels, metrics.heightPixels) - context.dp(32)
    return maxOf(1, available / (code.width + 8))
}

/** Draws a QR code into `pixels` (with a light border of 4 modules), each module `scale` pixels wide. */
fun drawQr(code: QrImage, scale: Int, pixels: IntArray) {
    val size = (code.width + 8) * scale
    Arrays.fill(pixels, Color.WHITE)
    for (my in 0 until code.width) {
        for (mx in 0 until code.width) {
            if (code.modules[my * code.width + mx].toInt() == 1) {
                val x0 = (mx + 4) * scale
                val y0 = (my + 4) * scale
                for (y in y0 until y0 + scale) {
                    val row = y * size
                    for (x in x0 until x0 + scale) pixels[row + x] = Color.BLACK
                }
            }
        }
    }
}

/**
 * A QR code as a picture to keep or print: sharp squares of `scale` pixels with a white border of 4
 * modules, and under it lines of text, each made to fit the width.
 */
fun qrPicture(code: QrImage, caption: List<String>, scale: Int = 12): Bitmap {
    val side = (code.width + 8) * scale
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.BLACK
        textSize = 28f
        textAlign = Paint.Align.CENTER
    }
    val lineHeight = 40
    val picture = Bitmap.createBitmap(side, side + caption.size * lineHeight + 16, Bitmap.Config.ARGB_8888)
    picture.eraseColor(Color.WHITE)
    val pixels = IntArray(side * side)
    drawQr(code, scale, pixels)
    picture.setPixels(pixels, 0, side, 0, 0, side, side)
    val canvas = Canvas(picture)
    // the text goes below the code and its white border, which cameras need to find the code
    var baseline = side + 30f
    for (line in caption) {
        canvas.drawText(fitted(line, paint, side - 4 * scale), side / 2f, baseline, paint)
        baseline += lineHeight
    }
    return picture
}

/** `text` as it fits into `width` pixels: whole, or cut with "...". */
private fun fitted(text: String, paint: Paint, width: Int): String {
    if (paint.measureText(text) <= width) return text
    var cut = text
    while (cut.isNotEmpty() && paint.measureText("$cut...") > width) cut = cut.dropLast(1)
    return "$cut..."
}

/** A still QR code, drawn with whole pixels per module (sharp, as cameras like it). */
fun qrView(context: Context, code: QrImage): ImageView {
    val scale = qrScale(context, code)
    val size = (code.width + 8) * scale
    val pixels = IntArray(size * size)
    drawQr(code, scale, pixels)
    val bitmap = Bitmap.createBitmap(pixels, size, size, Bitmap.Config.ARGB_8888)
    return ImageView(context).apply {
        setImageBitmap(bitmap)
        scaleType = ImageView.ScaleType.CENTER
    }
}
