package org.seedlock.app

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.ImageView
import org.seedlock.ffi.QrImage
import org.seedlock.ffi.SeedLockException
import org.seedlock.ffi.qrAnimation
import org.seedlock.ffi.qrCode

/**
 * Shows a QR code for another camera to read: one code, or an animated series (a signed transaction),
 * one code every 300 ms in a loop, as the command-line program shows them. The screen stays on and at
 * full brightness meanwhile. `note`, when given, stands out above the hint (where a multisig payment
 * stands, for example); `more` adds buttons below the code.
 */
class QrScreen(
    app: MainActivity,
    override val title: String,
    private val texts: List<String>,
    private val animated: Boolean,
    private val hint: String,
    private val note: String? = null,
    private val more: ((Page) -> Unit)? = null,
) : Screen(app) {
    private val handler = Handler(Looper.getMainLooper())
    private var codes: List<QrImage> = emptyList()
    private var frame = 0
    private var scale = 1
    private var image: ImageView? = null
    private var bitmap: Bitmap? = null
    private var pixels: IntArray? = null

    private val tick = object : Runnable {
        override fun run() {
            if (codes.isEmpty()) return
            frame = (frame + 1) % codes.size
            draw()
            handler.postDelayed(this, 300)
        }
    }

    override fun build(): View {
        val page = Page(app)
        note?.let { page.note(it) }
        page.text(hint)
        codes = try {
            if (texts.isEmpty()) throw SeedLockException.Failed("there is nothing to show")
            if (animated) qrAnimation(texts) else listOf(qrCode(texts[0]))
        } catch (e: SeedLockException) {
            page.warning("The QR code cannot be made: ${e.toUserText()}")
            more?.invoke(page)
            return page.view
        }
        scale = qrScale(app, codes[0])
        val size = (codes[0].width + 8) * scale
        val bmp = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        bitmap = bmp
        pixels = IntArray(size * size)
        image = page.add(ImageView(app).apply {
            setImageBitmap(bmp)
            scaleType = ImageView.ScaleType.CENTER
        }, 8)
        frame = 0
        draw()
        if (animated && codes.size > 1) page.small("${codes.size} QR codes, shown one after the other.")
        more?.invoke(page)
        return page.view
    }

    private fun draw() {
        val bmp = bitmap ?: return
        val px = pixels ?: return
        if (frame >= codes.size) return
        drawQr(codes[frame], scale, px)
        bmp.setPixels(px, 0, bmp.width, 0, 0, bmp.width, bmp.height)
        image?.invalidate()
    }

    override fun shown() {
        app.qrShowing(true)
        handler.removeCallbacks(tick)
        if (animated && codes.size > 1) handler.postDelayed(tick, 300)
    }

    override fun hidden() {
        handler.removeCallbacks(tick)
        app.qrShowing(false)
    }
}
