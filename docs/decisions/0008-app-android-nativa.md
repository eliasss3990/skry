# ADR 0008: App Android nativa (MediaProjection) como cliente de captura

- Estado: Aceptada
- Fecha: 2026-06-26

## Contexto

El server actual corre via `app_process` con uid shell, desplegado por `adb`
(ADR 0001, 0002). Sirvió para validar captura, encode H.265 por hardware y la
pantalla virtual independiente. Pero el transporte por `adb` mostró límites
operativos reales en uso:

- Tras un corte de luz, el teléfono cambia de IP (DHCP) y pierde el modo
  `adb tcpip`; hubo que reconectar por **cable** y rehacer el `tcpip`.
- Cada arranque exige tener la notebook delante (correr el comando, resolver la
  IP). No es cómodo para el caso de uso principal (ver contenido en la PC).
- Extender el sistema (UI, opciones, input) sobre un spike de reflection es
  frágil.

## Decisión

Construir una **app Android nativa en Kotlin** como el cliente de captura de
primera clase. El spike por `app_process` queda como camino alternativo/debug,
no como la experiencia principal.

### Pilares

1. **Captura via `MediaProjection`**, no reflection ni uid shell. El usuario
   concede el permiso de captura con un toque (una vez). Sin `adb`, sin
   `app_process`, sin TRUSTED hacks. Robusto ante updates de Android/One UI.
2. **Foreground service** con notificación persistente: la captura sobrevive
   aunque la app pase a segundo plano (es justamente el caso "dejá el celu y
   seguí transmitiendo a la PC").
3. **Descubrimiento sin IP fija**: la app expone el stream y se anuncia por
   **mDNS/NSD** (`_skry._tcp`). La PC la encuentra sola; inmune a cambios de IP.
   Fallback: mostrar un **QR** con el endpoint para emparejar a mano.
4. **Sin auto-actualización**: la app **chequea** si hay release nueva en GitHub
   y lo **avisa dentro de la app** (banner/notificación). La instalación la
   decide el usuario manualmente. Nunca se actualiza sola.
5. **UI moderna**: Jetpack Compose + Material 3 (dynamic color). Pantalla
   principal limpia: estado de conexión, botón de iniciar/detener captura,
   selector de modo (espejo / pantalla independiente), y aviso de update.

### Qué se conserva

- El **protocolo skry** (handshake + framing) y el cliente Rust (decode FFmpeg +
  render SDL2) no cambian: la app habla el mismo wire. El cliente de PC sigue
  siendo el mismo `.exe`.
- La pantalla virtual independiente (ADR sobre new-display) se reimplementa
  sobre la app, pero la idea y los flags del cliente se mantienen.

## Build y verificación

- Proyecto Gradle (Kotlin DSL) en `android-app/`. Build dockerizado
  (coherente con ADR 0004): imagen con Android SDK + Gradle pineados, sin
  binarios en git (se corre `gradle` de la imagen, no el wrapper jar).
- CI produce el **APK** como artefacto (verificable sin device). El runtime real
  sigue siendo el teléfono.

## Consecuencias

- El emparejamiento app↔PC pasa a ser por red local (mDNS/QR), no por túnel adb.
  El cliente Rust gana un modo "conectar a host:puerto descubierto".
- Se gana comodidad (cero cable, cero tocar la notebook) y robustez ante cortes.
- Costo: un subsistema nuevo (app + su build). Se hace por fases verificables;
  la primera es el esqueleto que compila a APK en CI.
