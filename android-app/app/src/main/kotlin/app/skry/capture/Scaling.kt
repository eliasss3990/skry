package app.skry.capture

/**
 * Escalado de dimensiones de captura. Aislado de [CaptureService] (sin imports de
 * Android) para poder testearlo como unidad JVM pura.
 */
object Scaling {

    /** Escala (w,h) para que el lado mayor no supere [maxDim], con dims pares. */
    fun scaleDown(w: Int, h: Int, maxDim: Int): Pair<Int, Int> {
        val longest = maxOf(w, h)
        if (longest <= maxDim) return evenPair(w, h)
        val f = maxDim.toDouble() / longest
        return evenPair((w * f).toInt(), (h * f).toInt())
    }

    private fun evenPair(w: Int, h: Int): Pair<Int, Int> =
        Pair(maxOf(2, w and 1.inv()), maxOf(2, h and 1.inv()))
}
