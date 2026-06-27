package app.skry.update

import android.os.Handler
import android.os.Looper
import android.util.Log
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/** Datos de una release más nueva que la instalada. */
data class UpdateInfo(val version: String, val url: String)

/**
 * Chequea si hay una release más nueva en GitHub y lo avisa. NO descarga ni
 * instala nada: la decisión de actualizar es del usuario (ADR 0008).
 *
 * Usa `HttpURLConnection` + `org.json` (ambos en el SDK), sin dependencias extra.
 */
object UpdateChecker {

    private const val TAG = "skry-update"
    private const val LATEST_RELEASE_URL =
        "https://api.github.com/repos/eliasss3990/skry/releases/latest"
    private const val TIMEOUT_MS = 5000

    /**
     * Consulta en segundo plano y entrega el resultado en el hilo main. [onResult]
     * recibe null si no hay update o si el chequeo falla (best-effort, no molesta).
     */
    fun checkAsync(currentVersion: String, onResult: (UpdateInfo?) -> Unit) {
        Thread({
            val info = runCatching { fetchLatest(currentVersion) }
                .onFailure { Log.i(TAG, "chequeo de update falló (${it::class.simpleName}): ${it.message}") }
                .getOrNull()
            Handler(Looper.getMainLooper()).post { onResult(info) }
        }, "skry-update-check").start()
    }

    private fun fetchLatest(current: String): UpdateInfo? {
        val conn = (URL(LATEST_RELEASE_URL).openConnection() as HttpURLConnection).apply {
            connectTimeout = TIMEOUT_MS
            readTimeout = TIMEOUT_MS
            setRequestProperty("Accept", "application/vnd.github+json")
        }
        try {
            if (conn.responseCode != HttpURLConnection.HTTP_OK) {
                Log.i(TAG, "GitHub respondió ${conn.responseCode}")
                return null
            }
            val body = conn.inputStream.bufferedReader().use { it.readText() }
            val json = JSONObject(body)
            val tag = json.optString("tag_name").removePrefix("v").trim()
            val url = json.optString("html_url")
            if (tag.isEmpty() || url.isEmpty()) return null
            return if (isNewer(tag, current)) UpdateInfo(tag, url) else null
        } finally {
            conn.disconnect()
        }
    }

    /**
     * Compara versiones tipo `MAJOR.MINOR.PATCH` numéricamente, componente a
     * componente. Las partes no numéricas o faltantes cuentan como 0.
     */
    fun isNewer(remote: String, current: String): Boolean {
        val r = parts(remote)
        val c = parts(current)
        val n = maxOf(r.size, c.size)
        for (i in 0 until n) {
            val rv = r.getOrElse(i) { 0 }
            val cv = c.getOrElse(i) { 0 }
            if (rv != cv) return rv > cv
        }
        return false
    }

    private fun parts(v: String): List<Int> =
        v.split('.').map { piece -> piece.takeWhile { it.isDigit() }.toIntOrNull() ?: 0 }
}
