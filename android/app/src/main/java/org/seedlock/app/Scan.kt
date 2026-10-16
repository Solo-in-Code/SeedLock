package org.seedlock.app

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.util.Size
import android.view.Gravity
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import org.seedlock.ffi.QrScanner
import org.seedlock.ffi.ScanTarget
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Reads a transaction, a backup or a multisig wallet with the phone's camera. Each camera picture's grey
 * part goes to seedlock-ffi's [QrScanner] (the command-line program's QR reader), on a thread of its own;
 * pictures that arrive while one is being read are dropped. When everything has been read, `done`
 * receives the scanner (whoever receives it closes it). Nothing from the camera is stored.
 */
class ScanScreen(app: MainActivity, private val target: ScanTarget, private val done: (QrScanner) -> Unit) : Screen(app) {
    override val title = when (target) {
        ScanTarget.TRANSACTION -> "Scan the transaction"
        ScanTarget.BACKUP -> "Scan the backup"
        ScanTarget.MULTISIG_WALLET -> "Scan the multisig wallet"
    }

    private var scanner: QrScanner? = QrScanner(target)
    private val analysis: ExecutorService = Executors.newSingleThreadExecutor()
    private var preview: TextureView? = null
    private var status: TextView? = null
    private var bar: ProgressBar? = null
    @Volatile private var camera: CameraDevice? = null
    @Volatile private var session: CameraCaptureSession? = null
    @Volatile private var reader: ImageReader? = null
    private var thread: HandlerThread? = null
    private var handler: Handler? = null

    /** Counted down when the camera asked for is open, or could not be opened (see [stopCamera]). */
    private var opening: CountDownLatch? = null

    @Volatile private var reading = false
    @Volatile private var finished = false
    @Volatile private var visible = false

    override fun build(): View {
        val page = Page(app)
        page.text(
            when (target) {
                ScanTarget.TRANSACTION ->
                    "In Sparrow, show the transaction as a QR code (Show QR), and hold this phone's camera toward that screen. " +
                        "The code is animated and changes all the time: that is normal."
                ScanTarget.BACKUP -> "Hold the backup's QR code (printed, or on another screen) in front of the camera, flat and steady."
                ScanTarget.MULTISIG_WALLET ->
                    "In Sparrow, open the multisig wallet's Settings tab and click the QR code button under Script Policy " +
                        "(\"Show as QR code\"), then hold this phone's camera toward that screen. The code may be animated: " +
                        "keep the camera on it until every part has arrived."
            }
        )
        val texture = TextureView(app)
        texture.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
            override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
                startCamera()
            }

            override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {}

            override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean {
                stopCamera()
                return true
            }

            override fun onSurfaceTextureUpdated(surface: SurfaceTexture) {}
        }
        preview = texture
        page.add(texture, 8)
        status = page.text("Opening the camera...")
        bar = page.add(ProgressBar(app, null, android.R.attr.progressBarStyleHorizontal).apply {
            visibility = View.GONE
        })
        page.button("Cancel") { app.back() }
        return page.view
    }

    override fun shown() {
        visible = true
        // the screen stays on while the camera looks for the code
        app.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (app.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            if (preview?.isAvailable == true) startCamera()
        } else {
            app.askForCamera()
        }
    }

    override fun permissionResult(granted: Boolean) {
        if (granted) {
            if (preview?.isAvailable == true) startCamera()
        } else {
            status?.text = "SeedLock may not use the camera. Allow it in the phone's Settings (Apps > SeedLock > " +
                "Permissions > Camera), or open the file instead."
        }
    }

    override fun hidden() {
        visible = false
        app.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        stopCamera()
    }

    override fun closed() {
        finished = true
        stopCamera()
        analysis.shutdown()
        scanner?.close()
        scanner = null
    }

    private fun say(text: String) {
        app.runOnUiThread { status?.text = text }
    }

    @SuppressLint("MissingPermission")
    private fun startCamera() {
        if (camera != null || opening?.count == 1L || finished || !visible) return
        if (app.checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) return
        val texture = preview ?: return
        val surfaceTexture = texture.surfaceTexture ?: return
        try {
            val manager = app.getSystemService(Context.CAMERA_SERVICE) as CameraManager
            val id = backCamera(manager) ?: return say("No camera was found on this phone. Open the file instead.")
            val characteristics = manager.getCameraCharacteristics(id)
            val map = characteristics.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
                ?: return say("The camera cannot be used (no picture sizes).")
            val size = pictureSize(map.getOutputSizes(ImageFormat.YUV_420_888), map.getOutputSizes(SurfaceTexture::class.java))
                ?: return say("The camera cannot be used (no suitable picture size).")
            val autofocus = characteristics.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)
                ?.contains(CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE) == true

            val t = HandlerThread("camera")
            t.start()
            val h = Handler(t.looper)
            thread = t
            handler = h
            val r = ImageReader.newInstance(size.width, size.height, ImageFormat.YUV_420_888, 2)
            r.setOnImageAvailableListener({ onPicture(it) }, h)
            reader = r
            surfaceTexture.setDefaultBufferSize(size.width, size.height)
            fitPreview(texture, size)

            val latch = CountDownLatch(1)
            opening = latch
            manager.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(device: CameraDevice) {
                    if (finished || !visible || reader !== r) {
                        // the screen was left while the camera was opening
                        device.close()
                        latch.countDown()
                        return
                    }
                    camera = device
                    latch.countDown()
                    startSession(device, Surface(surfaceTexture), r, h, autofocus)
                }

                override fun onDisconnected(device: CameraDevice) {
                    device.close()
                    if (camera == device) camera = null
                    latch.countDown()
                }

                override fun onError(device: CameraDevice, error: Int) {
                    device.close()
                    if (camera == device) camera = null
                    latch.countDown()
                    say("The camera could not be opened (error $error). Close other apps that use it, and try again.")
                }
            }, h)
            say("Camera is on. Looking for the QR code...")
        } catch (e: CameraAccessException) {
            opening?.countDown()
            say("The camera could not be opened: ${e.message}")
        } catch (e: SecurityException) {
            opening?.countDown()
            say("SeedLock may not use the camera. Allow it in the phone's Settings.")
        } catch (e: IllegalArgumentException) {
            opening?.countDown()
            say("The camera could not be opened: ${e.message}")
        }
    }

    private fun startSession(device: CameraDevice, previewSurface: Surface, r: ImageReader, h: Handler, autofocus: Boolean) {
        try {
            @Suppress("DEPRECATION")
            device.createCaptureSession(listOf(previewSurface, r.surface), object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(s: CameraCaptureSession) {
                    if (camera != device) {
                        s.close()
                        return
                    }
                    session = s
                    try {
                        val request = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                        request.addTarget(previewSurface)
                        request.addTarget(r.surface)
                        if (autofocus) {
                            request.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                        }
                        s.setRepeatingRequest(request.build(), null, h)
                    } catch (e: Exception) {
                        say("The camera could not start: ${e.message}")
                    }
                }

                override fun onConfigureFailed(s: CameraCaptureSession) {
                    say("The camera could not start.")
                }
            }, h)
        } catch (e: Exception) {
            say("The camera could not start: ${e.message}")
        }
    }

    /** A camera picture arrived (on the camera's thread): take its grey part, unless one is still being read. */
    private fun onPicture(r: ImageReader) {
        val picture = try {
            r.acquireLatestImage()
        } catch (e: IllegalStateException) {
            null
        } ?: return
        try {
            if (reading || finished) return
            val width = picture.width
            val height = picture.height
            val plane = picture.planes[0]
            val buffer = plane.buffer
            val rowStride = plane.rowStride
            val pixelStride = plane.pixelStride
            val luma = ByteArray(width * height)
            for (y in 0 until height) {
                if (pixelStride == 1) {
                    buffer.position(y * rowStride)
                    buffer.get(luma, y * width, width)
                } else {
                    for (x in 0 until width) luma[y * width + x] = buffer.get(y * rowStride + x * pixelStride)
                }
            }
            reading = true
            analysis.execute { read(width, height, luma) }
        } catch (e: Exception) {
            // a picture that cannot be read is skipped
        } finally {
            picture.close()
        }
    }

    /** Reads one picture (on the analysis thread) and shows what came of it. */
    private fun read(width: Int, height: Int, luma: ByteArray) {
        val s = scanner
        val step = try {
            s?.addPicture(width, height, luma)
        } catch (e: Exception) {
            null
        }
        app.runOnUiThread {
            reading = false
            if (step == null || finished || s == null) return@runOnUiThread
            step.message?.let { status?.text = it }
            if (step.total > 0) {
                bar?.max = step.total
                bar?.progress = step.have
                bar?.visibility = View.VISIBLE
            }
            if (step.done) {
                finished = true
                stopCamera()
                scanner = null // handed over: `done` closes it
                done(s)
            }
        }
    }

    private fun stopCamera() {
        // A camera that is still being opened is closed once it is open: Android takes a moment to open
        // it, and a camera closed before that would stay open (and its light on) until SeedLock ends.
        try {
            opening?.await(3, TimeUnit.SECONDS)
        } catch (e: InterruptedException) {
        }
        opening = null
        try {
            session?.close()
        } catch (e: Exception) {
        }
        session = null
        try {
            camera?.close()
        } catch (e: Exception) {
        }
        camera = null
        try {
            reader?.close()
        } catch (e: Exception) {
        }
        reader = null
        thread?.quitSafely()
        thread = null
        handler = null
    }

    /** The preview keeps the picture's proportions; the phone is upright, so the picture stands on its side. */
    private fun fitPreview(texture: TextureView, size: Size) {
        texture.post {
            val parent = texture.parent as? View
            val width = if (parent != null) parent.width - parent.paddingLeft - parent.paddingRight else texture.width
            if (width <= 0) return@post
            val maxHeight = (app.resources.displayMetrics.heightPixels * 0.55).toInt()
            var w = width
            var h = width * maxOf(size.width, size.height) / minOf(size.width, size.height)
            if (h > maxHeight) {
                h = maxHeight
                w = h * minOf(size.width, size.height) / maxOf(size.width, size.height)
            }
            val params = LinearLayout.LayoutParams(w, h)
            params.gravity = Gravity.CENTER_HORIZONTAL
            params.topMargin = app.dp(8)
            texture.layoutParams = params
        }
    }

    companion object {
        /** The camera on the back, else the first one. */
        fun backCamera(manager: CameraManager): String? {
            val ids = manager.cameraIdList
            for (id in ids) {
                val facing = manager.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING)
                if (facing == CameraCharacteristics.LENS_FACING_BACK) return id
            }
            return ids.firstOrNull()
        }

        /**
         * The picture size closest to 1280 x 720 that both the reader and the preview can have: large
         * enough for a dense code on a screen, small enough to read several pictures a second.
         */
        fun pictureSize(forReader: Array<Size>?, forPreview: Array<Size>?): Size? {
            val both = (forReader ?: return null).filter { r -> forPreview?.any { it.width == r.width && it.height == r.height } ?: true }
            val usable = both.filter { it.width <= 1920 && it.height <= 1920 && it.width * it.height >= 320 * 240 }
            val candidates = usable.ifEmpty { both }
            return candidates.minByOrNull { Math.abs(it.width * it.height - 1280 * 720) }
        }
    }
}
