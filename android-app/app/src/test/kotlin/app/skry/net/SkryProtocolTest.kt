package app.skry.net

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

/**
 * Verifica que la serialización del wire calce byte a byte con lo que espera el
 * cliente Rust (skry-proto), todo big-endian. Si esto se rompe, el cliente deja
 * de entender a la app.
 */
class SkryProtocolTest {

    private fun bytes(block: (DataOutputStream) -> Unit): ByteArray {
        val bos = ByteArrayOutputStream()
        DataOutputStream(bos).use(block)
        return bos.toByteArray()
    }

    @Test
    fun handshake_layout_exacto() {
        val out = bytes { SkryProtocol.writeHandshake(it, SkryProtocol.CODEC_H265, 1600, 900, "AB") }
        val expected = byteArrayOf(
            0x53, 0x4B, 0x52, 0x59, // "SKRY"
            0x00, 0x01, // version u16 = 1
            0x01, // codec H265
            0x06, 0x40, // width 1600
            0x03, 0x84.toByte(), // height 900
            0x00, 0x02, // strlen 2
            0x41, 0x42, // "AB"
        )
        assertArrayEquals(expected, out)
    }

    @Test
    fun frame_layout_exacto() {
        val payload = byteArrayOf(0xAA.toByte(), 0xBB.toByte())
        val out = bytes { SkryProtocol.writeFrame(it, 0x0102030405060708, SkryProtocol.FLAG_KEYFRAME, payload) }
        val expected = byteArrayOf(
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, // pts u64
            0x01, // flags keyframe
            0x00, 0x00, 0x00, 0x02, // len u32 = 2
            0xAA.toByte(), 0xBB.toByte(),
        )
        assertArrayEquals(expected, out)
    }

    @Test
    fun handshake_rechaza_dimension_fuera_de_u16() {
        assertThrows(IllegalArgumentException::class.java) {
            bytes { SkryProtocol.writeHandshake(it, SkryProtocol.CODEC_H265, 70000, 900, "x") }
        }
    }
}
