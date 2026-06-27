package app.skry.net

import java.net.Inet4Address
import java.net.NetworkInterface

/** Utilidades para mostrarle al usuario a qué dirección conectar la PC. */
object LocalAddress {

    /**
     * Primera IPv4 de red local (Wi-Fi/ethernet) no-loopback. La PC se conecta a
     * esta dirección con `skry --connect <ip>:7345`. No requiere permisos.
     * Devuelve null si no hay una interfaz con IPv4 utilizable.
     */
    fun wifiIpv4(): String? {
        return runCatching {
            NetworkInterface.getNetworkInterfaces().asSequence()
                .filter { it.isUp && !it.isLoopback && !it.isVirtual }
                .flatMap { it.inetAddresses.asSequence() }
                .filterIsInstance<Inet4Address>()
                .firstOrNull { it.isSiteLocalAddress }
                ?.hostAddress
        }.getOrNull()
    }
}
