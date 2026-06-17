package skry.proto

import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

/** Error de protocolo: cualquier desvío del formato. Nunca se interpreta basura como dato. */
class ProtocolException(message: String) : IOException(message)

/** Magic que abre el handshake: bytes ASCII de "SKRY". */
val MAGIC = byteArrayOf(0x53, 0x4B, 0x52, 0x59)

const val PROTOCOL_VERSION = 1
const val MAX_FRAME_BYTES = 16 * 1024 * 1024L
const val MAX_STRING_BYTES = 65535

/**
 * Helpers de wire en big-endian, espejo de `skry-proto` (Rust). Java/Kotlin no
 * tienen enteros sin signo nativos: los `u16`/`u32` se ensanchan a `Int`/`Long`
 * y se validan los rangos antes de cualquier reserva. UTF-8 es ESTÁNDAR (nunca
 * `writeUTF`/`readUTF`, que usan Modified UTF-8).
 */
object Wire {
    fun writeU8(out: DataOutputStream, v: Int) {
        require(v in 0..0xFF) { "u8 fuera de rango: $v" }
        out.writeByte(v)
    }

    fun readU8(inp: DataInputStream): Int = inp.readUnsignedByte()

    fun writeU16(out: DataOutputStream, v: Int) {
        require(v in 0..0xFFFF) { "u16 fuera de rango: $v" }
        out.writeShort(v)
    }

    fun readU16(inp: DataInputStream): Int = inp.readUnsignedShort()

    fun writeU32(out: DataOutputStream, v: Long) {
        require(v in 0..0xFFFFFFFFL) { "u32 fuera de rango: $v" }
        out.writeInt(v.toInt())
    }

    fun readU32(inp: DataInputStream): Long = inp.readInt().toLong() and 0xFFFFFFFFL

    fun writeU64(out: DataOutputStream, v: Long) = out.writeLong(v)

    fun readU64(inp: DataInputStream): Long = inp.readLong()

    fun writeString(out: DataOutputStream, s: String) {
        val bytes = s.toByteArray(StandardCharsets.UTF_8)
        if (bytes.size > MAX_STRING_BYTES) {
            throw ProtocolException("string excede $MAX_STRING_BYTES bytes: ${bytes.size}")
        }
        writeU16(out, bytes.size)
        out.write(bytes)
    }

    fun readString(inp: DataInputStream): String {
        val len = readU16(inp)
        val buf = ByteArray(len)
        inp.readFully(buf)
        // Decodificar en modo estricto: UTF-8 inválido es error de protocolo
        // (paridad con Rust, que rechaza; el String(_, UTF_8) de Kotlin
        // reemplazaría por U+FFFD silenciosamente).
        val decoder = StandardCharsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
        return try {
            decoder.decode(ByteBuffer.wrap(buf)).toString()
        } catch (e: CharacterCodingException) {
            throw ProtocolException("cadena UTF-8 invalida")
        }
    }
}
