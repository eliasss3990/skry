package app.skry.net

import java.io.DataOutputStream

/**
 * Constantes y serialización del wire skry. Espejo exacto de `skry-proto` (cliente
 * Rust): todo big-endian. Si cambia el wire de un lado, cambiar acá también.
 */
object SkryProtocol {
    /** "SKRY". */
    val MAGIC = byteArrayOf(0x53, 0x4B, 0x52, 0x59)

    const val PROTOCOL_VERSION = 1
    const val CODEC_H265 = 1

    const val STREAM_VIDEO = 0x00
    const val STREAM_CONTROL = 0x01

    const val FLAG_KEYFRAME = 0x01
    const val FLAG_CODEC_CONFIG = 0x02

    /**
     * Handshake del canal de video:
     * magic(4) + version(u16) + codec(u8) + width(u16) + height(u16) + device(u16 len + utf8).
     */
    fun writeHandshake(out: DataOutputStream, codec: Int, width: Int, height: Int, deviceName: String) {
        require(width in 1..0xFFFF) { "width fuera de rango u16: $width" }
        require(height in 1..0xFFFF) { "height fuera de rango u16: $height" }
        out.write(MAGIC)
        out.writeShort(PROTOCOL_VERSION)
        out.writeByte(codec)
        out.writeShort(width)
        out.writeShort(height)
        writeString(out, deviceName)
        out.flush()
    }

    /** Frame: pts(u64) + flags(u8) + len(u32) + payload. */
    fun writeFrame(out: DataOutputStream, ptsUs: Long, flags: Int, payload: ByteArray) {
        writeFrame(out, ptsUs, flags, payload, payload.size)
    }

    /**
     * Igual que el anterior pero escribe sólo los primeros [len] bytes de
     * [payload]. Permite reusar un buffer más grande entre frames sin recortarlo.
     */
    fun writeFrame(out: DataOutputStream, ptsUs: Long, flags: Int, payload: ByteArray, len: Int) {
        require(len in 0..payload.size) { "len=$len fuera de rango [0, ${payload.size}]" }
        out.writeLong(ptsUs)
        out.writeByte(flags)
        out.writeInt(len)
        out.write(payload, 0, len)
        out.flush()
    }

    private fun writeString(out: DataOutputStream, s: String) {
        val bytes = s.toByteArray(Charsets.UTF_8)
        // El nombre del modelo nunca se acerca a u16::MAX; recorte defensivo igual.
        val safe = if (bytes.size > 0xFFFF) bytes.copyOf(0xFFFF) else bytes
        out.writeShort(safe.size)
        out.write(safe)
    }
}
