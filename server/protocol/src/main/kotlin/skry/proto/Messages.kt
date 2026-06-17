package skry.proto

import java.io.DataInputStream
import java.io.DataOutputStream

/** Primer mensaje del canal de video. Espejo de `skry_proto::Handshake`. */
data class Handshake(
    val codec: Codec,
    val width: Int,
    val height: Int,
    val deviceName: String,
) {
    fun write(out: DataOutputStream) {
        out.write(MAGIC)
        Wire.writeU16(out, PROTOCOL_VERSION)
        Wire.writeU8(out, codec.id)
        Wire.writeU16(out, width)
        Wire.writeU16(out, height)
        Wire.writeString(out, deviceName)
    }

    companion object {
        fun read(inp: DataInputStream): Handshake {
            val magic = ByteArray(4)
            inp.readFully(magic)
            if (!magic.contentEquals(MAGIC)) {
                throw ProtocolException("magic de handshake invalido")
            }
            val version = Wire.readU16(inp)
            if (version != PROTOCOL_VERSION) {
                throw ProtocolException("version incompatible: cliente v$PROTOCOL_VERSION, server v$version")
            }
            val codec = Codec.fromU8(Wire.readU8(inp))
            val width = Wire.readU16(inp)
            val height = Wire.readU16(inp)
            val deviceName = Wire.readString(inp)
            return Handshake(codec, width, height, deviceName)
        }
    }
}

private const val FLAG_KEYFRAME = 0x01
private const val FLAG_CONFIG = 0x02

/** Cabecera de un paquete de frame. Espejo de `skry_proto::FrameHeader`. */
data class FrameHeader(
    val pts: Long,
    val keyframe: Boolean,
    val config: Boolean,
    val len: Long,
) {
    private fun flags(): Int {
        var f = 0
        if (keyframe) f = f or FLAG_KEYFRAME
        if (config) f = f or FLAG_CONFIG
        return f
    }

    fun write(out: DataOutputStream) {
        if (len > MAX_FRAME_BYTES) {
            throw ProtocolException("frame fuera de rango: $len (max $MAX_FRAME_BYTES)")
        }
        Wire.writeU64(out, pts)
        Wire.writeU8(out, flags())
        Wire.writeU32(out, len)
    }

    companion object {
        fun read(inp: DataInputStream): FrameHeader {
            val pts = Wire.readU64(inp)
            val flags = Wire.readU8(inp)
            val len = Wire.readU32(inp)
            if (len > MAX_FRAME_BYTES) {
                throw ProtocolException("frame fuera de rango: $len (max $MAX_FRAME_BYTES)")
            }
            return FrameHeader(
                pts = pts,
                keyframe = flags and FLAG_KEYFRAME != 0,
                config = flags and FLAG_CONFIG != 0,
                len = len,
            )
        }
    }
}

data class Telemetry(val encodedFrames: Long, val droppedFrames: Long, val bitrate: Long)

/** Mensaje del cliente hacia el server. Espejo de `skry_proto::ClientMessage`. */
sealed class ClientMessage {
    data class SetGear(val gear: Gear) : ClientMessage()
    data class SetBitrate(val bitrate: Long) : ClientMessage()
    data class Ping(val seq: Long) : ClientMessage()
    data object Stop : ClientMessage()

    fun write(out: DataOutputStream) {
        when (this) {
            is SetGear -> { Wire.writeU8(out, 0x01); Wire.writeU8(out, gear.id) }
            is SetBitrate -> { Wire.writeU8(out, 0x02); Wire.writeU32(out, bitrate) }
            is Ping -> { Wire.writeU8(out, 0x03); Wire.writeU32(out, seq) }
            is Stop -> Wire.writeU8(out, 0x04)
        }
    }

    companion object {
        fun read(inp: DataInputStream): ClientMessage = when (val tag = Wire.readU8(inp)) {
            0x01 -> SetGear(Gear.fromU8(Wire.readU8(inp)))
            0x02 -> SetBitrate(Wire.readU32(inp))
            0x03 -> Ping(Wire.readU32(inp))
            0x04 -> Stop
            else -> throw ProtocolException("tag de ClientMessage desconocido: $tag")
        }
    }
}

/** Mensaje del server hacia el cliente. Espejo de `skry_proto::ServerMessage`. */
sealed class ServerMessage {
    data class Pong(val seq: Long) : ServerMessage()
    data class Tele(val telemetry: Telemetry) : ServerMessage()
    data class GearChanged(val gear: Gear) : ServerMessage()
    data class Error(val code: Int, val message: String) : ServerMessage()

    fun write(out: DataOutputStream) {
        when (this) {
            is Pong -> { Wire.writeU8(out, 0x81); Wire.writeU32(out, seq) }
            is Tele -> {
                Wire.writeU8(out, 0x82)
                Wire.writeU64(out, telemetry.encodedFrames)
                Wire.writeU64(out, telemetry.droppedFrames)
                Wire.writeU32(out, telemetry.bitrate)
            }
            is GearChanged -> { Wire.writeU8(out, 0x83); Wire.writeU8(out, gear.id) }
            is Error -> { Wire.writeU8(out, 0x84); Wire.writeU16(out, code); Wire.writeString(out, message) }
        }
    }

    companion object {
        // Tags del server con bit alto (0x81+): leer como u8 sin signo, no como
        // byte (que daria negativo y nunca matchearia).
        fun read(inp: DataInputStream): ServerMessage = when (val tag = Wire.readU8(inp)) {
            0x81 -> Pong(Wire.readU32(inp))
            0x82 -> Tele(Telemetry(Wire.readU64(inp), Wire.readU64(inp), Wire.readU32(inp)))
            0x83 -> GearChanged(Gear.fromU8(Wire.readU8(inp)))
            0x84 -> Error(Wire.readU16(inp), Wire.readString(inp))
            else -> throw ProtocolException("tag de ServerMessage desconocido: $tag")
        }
    }
}
