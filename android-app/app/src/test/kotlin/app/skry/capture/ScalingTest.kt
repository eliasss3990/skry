package app.skry.capture

import org.junit.Assert.assertEquals
import org.junit.Test

class ScalingTest {

    @Test
    fun no_escala_si_entra_en_el_limite() {
        assertEquals(Pair(1080, 1920), Scaling.scaleDown(1080, 1920, 2400))
    }

    @Test
    fun escala_el_lado_mayor_al_limite() {
        // 1440x3120, max 2400 -> lado mayor queda en 2400; el menor escala y se
        // hace par: 1440*2400/3120 = 1107.69 -> 1107 -> par 1106.
        val (w, h) = Scaling.scaleDown(1440, 3120, 2400)
        assertEquals(2400, h)
        assertEquals(1106, w)
    }

    @Test
    fun dimensiones_siempre_pares() {
        val (w, h) = Scaling.scaleDown(1079, 1921, 1000)
        assertEquals(0, w and 1)
        assertEquals(0, h and 1)
    }

    @Test
    fun nunca_baja_de_dos() {
        val (w, h) = Scaling.scaleDown(1, 1, 1)
        assertEquals(2, w)
        assertEquals(2, h)
    }
}
