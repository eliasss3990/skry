package skry.proto

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class WireTest {
    private fun roundTrip(write: (DataOutputStream) -> Unit): DataInputStream {
        val bos = ByteArrayOutputStream()
        write(DataOutputStream(bos))
        return DataInputStream(ByteArrayInputStream(bos.toByteArray()))
    }

    @Test
    fun handshakeRoundTrip() {
        val hs = Handshake(Codec.H265, 1440, 3120, "SM-S928B")
        val back = Handshake.read(roundTrip { hs.write(it) })
        assertEquals(hs, back)
    }

    @Test
    fun handshakeRejectsBadMagic() {
        val din = roundTrip { out ->
            out.write(byteArrayOf('X'.code.toByte(), 'X'.code.toByte(), 'X'.code.toByte(), 'X'.code.toByte()))
            Wire.writeU16(out, PROTOCOL_VERSION)
        }
        assertFailsWith<ProtocolException> { Handshake.read(din) }
    }

    @Test
    fun handshakeRejectsVersionMismatch() {
        // Espejo de handshake_rejects_version_mismatch (Rust).
        val din = roundTrip { out ->
            out.write(MAGIC)
            Wire.writeU16(out, PROTOCOL_VERSION + 1)
        }
        assertFailsWith<ProtocolException> { Handshake.read(din) }
    }

    @Test
    fun enumsRejectUnknownDiscriminant() {
        assertFailsWith<ProtocolException> { Codec.fromU8(99) }
        assertFailsWith<ProtocolException> { Gear.fromU8(99) }
        assertFailsWith<ProtocolException> { StreamType.fromU8(99) }
    }

    @Test
    fun frameReservedFlagBitsAreIgnored() {
        // Bits 2-7 reservados: un frame que los trae seteados debe decodificar
        // sin error, preservando keyframe/config de los bits 0-1.
        val din = roundTrip { out ->
            Wire.writeU64(out, 7)
            Wire.writeU8(out, 0xFD) // 1111_1101: config(bit1)=0, keyframe(bit0)=1, resto reservado
            Wire.writeU32(out, 0)
        }
        val h = FrameHeader.read(din)
        assertTrue(h.keyframe)
        assertTrue(!h.config)
        assertEquals(0L, h.len)
    }

    @Test
    fun frameRoundTripAndFlags() {
        val h = FrameHeader(pts = 1_234_567, keyframe = true, config = false, len = 0)
        val back = FrameHeader.read(roundTrip { h.write(it) })
        assertEquals(h, back)
        assertTrue(back.keyframe)
        assertTrue(!back.config)
    }

    @Test
    fun frameRejectsOversizedLen() {
        // Antiregresion portada de Rust (frame_rejects_oversized_len): un len
        // por encima del maximo debe ser error, no una reserva gigante.
        val din = roundTrip { out ->
            Wire.writeU64(out, 0)            // pts
            Wire.writeU8(out, 0)             // flags
            out.writeInt((MAX_FRAME_BYTES + 1).toInt()) // len crudo
        }
        assertFailsWith<ProtocolException> { FrameHeader.read(din) }
    }

    @Test
    fun clientMessagesRoundTrip() {
        val msgs = listOf(
            ClientMessage.SetGear(Gear.HIGH),
            ClientMessage.SetBitrate(8_000_000),
            ClientMessage.Ping(42),
            ClientMessage.Stop,
        )
        for (m in msgs) {
            assertEquals(m, ClientMessage.read(roundTrip { m.write(it) }))
        }
    }

    @Test
    fun serverMessagesRoundTrip() {
        val msgs = listOf(
            ServerMessage.Pong(42),
            ServerMessage.Tele(Telemetry(1000, 3, 6_000_000)),
            ServerMessage.GearChanged(Gear.LOW),
            ServerMessage.Error(7, "encoder no disponible"),
        )
        for (m in msgs) {
            assertEquals(m, ServerMessage.read(roundTrip { m.write(it) }))
        }
    }

    @Test
    fun serverErrorEmptyMessage() {
        val m = ServerMessage.Error(0, "")
        assertEquals(m, ServerMessage.read(roundTrip { m.write(it) }))
    }

    @Test
    fun streamTypeRoundTrip() {
        for (s in StreamType.entries) {
            assertEquals(s, StreamType.read(roundTrip { s.write(it) }))
        }
    }

    @Test
    fun unknownTagIsProtocolError() {
        val din = roundTrip { Wire.writeU8(it, 0xFF) }
        assertFailsWith<ProtocolException> { ClientMessage.read(din) }
    }

    @Test
    fun stringRejectsInvalidUtf8() {
        // Bytes UTF-8 invalidos: 0xFF no es un inicio valido. Debe rechazarse
        // (paridad con Rust), no reemplazarse silenciosamente por U+FFFD.
        val din = roundTrip { out ->
            Wire.writeU16(out, 1)
            out.writeByte(0xFF)
        }
        assertFailsWith<ProtocolException> { Wire.readString(din) }
    }

    @Test
    fun handshakeByteLayoutMatchesWire() {
        // Paridad exacta con el wire de Rust: magic + version(u16 BE) + codec +
        // width(u16 BE) + height(u16 BE) + len(u16 BE) + nombre.
        val bos = ByteArrayOutputStream()
        Handshake(Codec.H264, 0x0102, 0x0304, "AB").write(DataOutputStream(bos))
        val expected = byteArrayOf(
            0x53, 0x4B, 0x52, 0x59,       // "SKRY"
            0x00, 0x01,                   // version = 1
            0x00,                         // codec H264 = 0
            0x01, 0x02,                   // width = 0x0102
            0x03, 0x04,                   // height = 0x0304
            0x00, 0x02,                   // len("AB") = 2
            0x41, 0x42,                   // "AB"
        )
        assertTrue(expected.contentEquals(bos.toByteArray()))
    }
}
