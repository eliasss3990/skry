package skry.proto

/** Códec de video. Espejo de `skry_proto::Codec`. */
enum class Codec(val id: Int, val codecName: String) {
    H264(0, "h264"),
    H265(1, "h265");

    companion object {
        fun fromU8(v: Int): Codec = entries.firstOrNull { it.id == v }
            ?: throw ProtocolException("valor desconocido para Codec: $v")
    }
}

/** Marcha de fluidez. Espejo de `skry_proto::Gear`. */
enum class Gear(val id: Int, val targetFps: Int) {
    LOW(0, 60),
    MID(1, 120),
    HIGH(2, 144);

    companion object {
        fun fromU8(v: Int): Gear = entries.firstOrNull { it.id == v }
            ?: throw ProtocolException("valor desconocido para Gear: $v")
    }
}

/** Tipo de canal declarado por el cliente como primer byte de cada socket. */
enum class StreamType(val id: Int) {
    VIDEO(0x00),
    CONTROL(0x01);

    fun write(out: java.io.DataOutputStream) = Wire.writeU8(out, id)

    companion object {
        fun fromU8(v: Int): StreamType = entries.firstOrNull { it.id == v }
            ?: throw ProtocolException("valor desconocido para StreamType: $v")

        fun read(inp: java.io.DataInputStream): StreamType = fromU8(Wire.readU8(inp))
    }
}
