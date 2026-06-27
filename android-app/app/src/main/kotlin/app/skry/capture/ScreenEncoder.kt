package app.skry.capture

import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.projection.MediaProjection
import android.view.Surface
import app.skry.net.SkryProtocol

/**
 * Codifica la pantalla capturada por [MediaProjection] a H.265 por hardware, igual
 * que el spike validado en device: encoder con superficie de entrada + una pantalla
 * virtual que vuelca el contenido proyectado en esa superficie.
 *
 * Un encoder por sesión de cliente (se crea al conectar, se libera al desconectar).
 */
class ScreenEncoder(
    private val projection: MediaProjection,
    private val width: Int,
    private val height: Int,
    private val dpi: Int,
) {
    private var codec: MediaCodec? = null
    private var virtualDisplay: VirtualDisplay? = null
    private var inputSurface: Surface? = null

    fun start() {
        val format = MediaFormat.createVideoFormat(MIME, width, height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
            setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, I_FRAME_INTERVAL)
        }
        val c = MediaCodec.createEncoderByType(MIME)
        c.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        val surface = c.createInputSurface()
        c.start()
        // Asignar los campos ANTES de crear el virtual display: si createVirtualDisplay
        // tira, release() ya puede limpiar el codec y la surface (si no, fugarían).
        codec = c
        inputSurface = surface
        virtualDisplay = projection.createVirtualDisplay(
            "skry",
            width,
            height,
            dpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_PUBLIC,
            surface,
            null,
            null,
        )
    }

    /**
     * Drena el encoder y entrega cada frame por [onFrame] hasta que [shouldStop]
     * devuelva true o [onFrame] tire (p. ej. el cliente cortó la conexión).
     * Bloqueante: llamar desde un hilo dedicado.
     */
    fun drain(
        onFrame: (ptsUs: Long, flags: Int, payload: ByteArray, len: Int) -> Unit,
        shouldStop: () -> Boolean,
    ) {
        val c = codec ?: error("encoder no iniciado")
        val info = MediaCodec.BufferInfo()
        // Buffer reusado entre frames: a 60fps evitar una asignación por frame
        // ahorra presión de GC real. Crece sólo si un frame es más grande.
        var scratch = ByteArray(0)
        while (!shouldStop()) {
            val idx = c.dequeueOutputBuffer(info, DEQUEUE_TIMEOUT_US)
            if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
            if (idx < 0) continue
            val buf = c.getOutputBuffer(idx)
            if (buf != null && info.size > 0) {
                if (scratch.size < info.size) scratch = ByteArray(info.size)
                buf.position(info.offset)
                buf.limit(info.offset + info.size)
                buf.get(scratch, 0, info.size)

                var flags = 0
                if (info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0) {
                    flags = flags or SkryProtocol.FLAG_KEYFRAME
                }
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    flags = flags or SkryProtocol.FLAG_CODEC_CONFIG
                }
                onFrame(info.presentationTimeUs, flags, scratch, info.size)
            }
            c.releaseOutputBuffer(idx, false)
        }
    }

    fun release() {
        runCatching { virtualDisplay?.release() }
        runCatching { codec?.stop() }
        runCatching { codec?.release() }
        runCatching { inputSurface?.release() }
        virtualDisplay = null
        codec = null
        inputSurface = null
    }

    companion object {
        private const val MIME = MediaFormat.MIMETYPE_VIDEO_HEVC
        private const val BITRATE = 40_000_000
        private const val FRAME_RATE = 60
        private const val I_FRAME_INTERVAL = 1
        private const val DEQUEUE_TIMEOUT_US = 100_000L
    }
}
