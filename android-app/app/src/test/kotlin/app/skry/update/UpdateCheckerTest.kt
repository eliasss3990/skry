package app.skry.update

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateCheckerTest {

    @Test
    fun detecta_version_mas_nueva() {
        assertTrue(UpdateChecker.isNewer("0.2.0", "0.1.0"))
        assertTrue(UpdateChecker.isNewer("1.0.0", "0.9.9"))
        assertTrue(UpdateChecker.isNewer("0.10.0", "0.9.0")) // numérico, no lexicográfico
    }

    @Test
    fun misma_o_anterior_no_es_update() {
        assertFalse(UpdateChecker.isNewer("0.1.0", "0.1.0"))
        assertFalse(UpdateChecker.isNewer("0.1.0", "0.2.0"))
        assertFalse(UpdateChecker.isNewer("1.0", "1.0.0")) // parte faltante = 0
    }

    @Test
    fun tolera_sufijos_no_numericos() {
        // "1.0.0-beta" -> partes [1,0,0]; igual a "1.0.0" -> no es update.
        assertFalse(UpdateChecker.isNewer("1.0.0-beta", "1.0.0"))
        assertTrue(UpdateChecker.isNewer("1.2.0-rc1", "1.1.0"))
    }
}
