package app.skry.discovery

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.util.Log

/**
 * Anuncia el servidor skry por mDNS/DNS-SD (`_skry._tcp`) para que la PC lo
 * descubra sola, sin IP fija. Resuelve el dolor de los cambios de IP por DHCP.
 *
 * Toma un MulticastLock mientras anuncia: sin él, muchos chipsets WiFi filtran el
 * tráfico multicast y los paquetes mDNS nunca salen al cable (el servicio se
 * registra en el stack local pero los peers no lo ven).
 */
class NsdAdvertiser(context: Context) {

    private val appContext = context.applicationContext
    private val nsd = appContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val wifi = appContext.getSystemService(Context.WIFI_SERVICE) as WifiManager

    @Volatile
    private var listener: NsdManager.RegistrationListener? = null
    private var multicastLock: WifiManager.MulticastLock? = null

    /** Registra el servicio en [port] con [serviceName]. Idempotente best-effort. */
    fun register(port: Int, serviceName: String) {
        if (listener != null) return

        multicastLock = wifi.createMulticastLock("skry-mdns").apply {
            setReferenceCounted(false)
            runCatching { acquire() }
        }

        val info = NsdServiceInfo().apply {
            this.serviceName = serviceName
            serviceType = SERVICE_TYPE
            setPort(port)
        }
        val l = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                Log.i(TAG, "anunciado como ${info.serviceName} en $SERVICE_TYPE")
            }

            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.w(TAG, "registro mDNS falló (code=$errorCode)")
                // Liberar el listener y el lock para permitir un reintento posterior.
                listener = null
                releaseLock()
            }

            override fun onServiceUnregistered(info: NsdServiceInfo) {
                Log.i(TAG, "des-anunciado")
            }

            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.w(TAG, "des-registro mDNS falló (code=$errorCode)")
            }
        }
        listener = l
        nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, l)
    }

    fun unregister() {
        listener?.let { runCatching { nsd.unregisterService(it) } }
        listener = null
        releaseLock()
    }

    private fun releaseLock() {
        multicastLock?.let { lock ->
            runCatching { if (lock.isHeld) lock.release() }
        }
        multicastLock = null
    }

    companion object {
        private const val TAG = "skry-nsd"
        const val SERVICE_TYPE = "_skry._tcp."
    }
}
