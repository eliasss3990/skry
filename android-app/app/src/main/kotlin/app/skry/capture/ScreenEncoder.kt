package app.skry.capture

import android.hardware.display.VirtualDisplay
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.view.Surface
import app.skry.net.SkryProtocol

/**
 * Codec HEVC por hardware para UN cliente. NO crea la pantalla virtual: la recibe
 * de [CaptureService] y le redirige la salida con `setSurface`.
 *
 * Esto es clave en Android 14+: MediaProjection permite crear UNA sola
 * VirtualDisplay por instancia. Crear una por cliente (lo que se hacía antes)
 * rompía la proyección entera al conectar el segundo cliente. Ahora la pantalla
 * se crea una vez en el servicio y se reusa entre clientes; cada cliente solo
 * aporta su propio codec y lo engancha a la pantalla.
 */
class ScreenEncoder(
    private val virtualDisplay: VirtualDisplay,
    private val width: Int,
    private val height: Int,
) {
    private var codec: MediaCodec? = null
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
        // Asignar los campos ANTES de enganchar la pantalla: si algo falla después,
        // release() puede limpiar el codec y la surface igual.
        codec = c
        inputSurface = surface
        // Redirigir el mirror de la proyección a este codec.
        virtualDisplay.surface = surface
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
        // Soltar la pantalla virtual de este codec ANTES de liberarlo: la pantalla
        // la sigue siendo dueña el servicio y se reusa con el próximo cliente.
        runCatching { virtualDisplay.surface = null }
        runCatching { codec?.stop() }
        runCatching { codec?.release() }
        runCatching { inputSurface?.release() }
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
