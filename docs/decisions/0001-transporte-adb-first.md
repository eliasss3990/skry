# ADR 0001: Transporte sobre túnel ADB como base; Wi-Fi Direct opcional

- Estado: Aceptada
- Fecha: 2026-06-16

## Contexto

El plan original proponía **Wi-Fi Direct (P2P)** como transporte primario,
presentado como un "túnel inmune a la congestión del router". El video iría por
UDP/SRT sobre ese túnel P2P, con fallback a LAN y luego a 2.4GHz.

Al evaluar la viabilidad aparecen problemas estructurales:

1. **El server corre como uid `shell` (2000) vía `app_process`, sin contexto de
   aplicación.** `WifiP2pManager` requiere permisos de ubicación en runtime, un
   `BroadcastReceiver` registrado y, en la práctica, una app en foreground para
   conducir la formación del grupo P2P. Manejar esto desde un `.jar` pelado es
   inviable o extremadamente frágil.
2. **`Windows.Devices.WiFiDirect` depende del driver** y es históricamente
   inestable entre fabricantes de adaptadores.
3. **Si ya hay conexión ADB (USB o TCP), ya existe un canal confiable,
   autenticado y sin NAT.** El túnel P2P agrega complejidad enorme para un
   beneficio marginal, y su bootstrap (cómo se descubren PC y teléfono sobre el
   enlace P2P) igual necesita un canal de señalización.

## Decisión

El **transporte base del MVP es el túnel ADB** (`adb forward`/`adb reverse`
sobre TCP), igual que hace `scrcpy`. Es confiable, ya autenticado por la
autorización de depuración USB, y libre de NAT.

El transporte se define **detrás de la abstracción `skry-transport`**, de modo
que Wi-Fi Direct / LAN directa puedan incorporarse después como una
implementación alternativa **sin reescribir** el cliente ni el server.

Sobre el túnel ADB (que es orientado a stream y confiable) el video viaja por un
socket TCP. SRT/UDP quedan reservados para el futuro transporte P2P/LAN, donde
sí aportan (control de congestión propio sobre un medio con pérdidas).

## Consecuencias

- **Positivas**: el MVP funciona de forma robusta sobre un canal probado; menos
  superficie de fallo; la resiliencia se concentra en casos reales y testeables;
  el diseño no se cierra a Wi-Fi Direct, sólo lo pospone.
- **Negativas**: se difiere el "túnel P2P inmune al router" que era un atractivo
  del plan. Mitigación: la abstracción de transporte deja la puerta abierta;
  cuando se priorice, se implementa como capa nueva, no como reescritura.
- **Negativas**: sobre TCP, una pérdida puntual puede agregar latencia por
  retransmisión. Aceptable sobre el enlace USB/local; cuando importe, el
  transporte P2P con SRT lo direcciona.
