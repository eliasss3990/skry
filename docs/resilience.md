# Resiliencia y casos borde

Principio rector: **el programa nunca explota en silencio. Si puede
auto-recuperarse, lo hace. Si no puede, le dice al usuario exactamente qué
hacer.**

Cada mensaje al usuario sigue el formato: una línea de diagnóstico y, cuando
aplica, una línea de solución accionable.

## 1. Conexión física (dominio de ADB)

| Caso | Detección | Comportamiento |
|------|-----------|----------------|
| **A — Sin dispositivo** | `adb get-state` falla o vacío | Aborta inmediato con instrucción: conectar por USB/Wi-Fi y activar Depuración USB. |
| **B — Múltiples dispositivos** | `adb` devuelve `more than one device/emulator` | Parsea `adb devices`, lista los seriales y pide reintentar con `--serial <ID>`. |
| **C — No autorizado** | estado `unauthorized` | No cierra: entra en espera (loop de ~10s) pidiendo aceptar el diálogo en el teléfono. |

## 2. Red (dominio del transporte)

> En el MVP el transporte es el túnel ADB (ADR-0001), que es confiable. Los
> fallbacks de Wi-Fi Direct / banda aplican cuando se incorpore el transporte
> P2P. Se documentan acá para no perder el diseño.

| Caso | Comportamiento |
|------|----------------|
| **A — Sin 5GHz o banda saturada** | Fallback transparente a 2.4GHz P2P; informativo, no bloquea. |
| **B — Wi-Fi Direct no disponible** | Fallback a LAN: detectar IP del teléfono (`adb shell ip route`) y streamear por la red local. Aviso de posible mayor latencia. |

## 3. Hardware de decodificación (GPU)

| Caso | Comportamiento |
|------|----------------|
| **A — Sin GPU dedicada / drivers viejos** | Al fallar la init de hwaccel (DXVA2/NVDEC/VAAPI), el cliente destruye el decoder y levanta uno por CPU (software). Aviso de mayor uso de CPU. El fallback es transparente: el stream no se corta. |

## 4. Runtime (durante el stream)

| Caso | Detección | Comportamiento |
|------|-----------|----------------|
| **A — Throttling térmico del teléfono** | El canal de control nota que la latencia se dispara y suben los frames perdidos | Fuerza la marcha inferior (hasta 60 FPS) de forma agresiva. Aviso al usuario. |
| **B — Caída de conexión / túnel roto** | El canal de video no recibe datos por ~3s | Entra en modo reconexión (hasta 3 intentos). Si falla, cierra la ventana con gracia, mata el proceso en Android por seguridad y termina. |

## Cierre con gracia

Cualquier terminación (error fatal, Ctrl-C, fin de reconexión) debe:

1. Cerrar la ventana de render sin dejarla colgada.
2. Matar el proceso `app_process` en el teléfono (no dejar el server huérfano
   consumiendo batería).
3. Liberar los `forward`/`reverse` de ADB creados por la sesión.
4. Devolver un código de salida distinto de cero ante error, cero ante cierre
   normal.
