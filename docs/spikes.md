# Plan de validación en dispositivo real (spikes)

El corazón del riesgo de skry no es el código ya escrito (protocolo, adb), sino
la **captura de pantalla en Android** sobre el dispositivo de validación: un
Samsung S24 Ultra (One UI 8, Android 16), de los entornos más hostiles para
esta técnica (scrcpy tiene issues abiertos sin fix en este hardware).

Por eso, **antes de construir el pipeline completo** (encode → red → decode →
render), validamos en 3 spikes aislados, en orden de riesgo. Cada uno descarta
una incógnita sin arrastrar las de los siguientes. Referencia: ADR-0002.

> Regla de oro: **no avanzar al siguiente spike hasta que el anterior pase.**
> Descubrir un problema de captura después de armar todo el pipeline cuesta días;
> descubrirlo con un PNG cuesta minutos.

## Dispositivo de validación (confirmado)

Samsung Galaxy S24 Ultra `SM-S928B` (serial real omitido):

- **Android 16** (`ro.build.version.release=16`), **API 36** (`sdk=36`).
- **One UI 8** (`ro.build.version.oneui=80500`).
- `adb shell id` → `uid=2000(shell)` ✓ (base para `app_process`).

Es más nuevo que lo que asumía el pre-mortem (14/15): es el extremo bleeding-edge.
`SurfaceControl.createDisplay` está removido con seguridad; las firmas internas
pueden haber cambiado en 16/One UI 8. El camino de captura apunta a API 36 con
`DisplayManager.createVirtualDisplay` por reflexión + enumeración de métodos ante
fallo.

## Pre-flight del dispositivo (5 min)

Antes de cualquier spike, en el S24 (por **USB**, no Wi-Fi todavía — ver R4):

- [ ] Opciones de desarrollador → **Depuración USB** ON.
- [ ] **Instalar via USB** ON.
- [ ] **Auto Blocker** OFF (One UI 6+ puede bloquear sideload).
- [ ] **Permanecer activo** (Stay awake) ON, pantalla encendida.
- [ ] `adb shell id` → confirmar `uid=2000(shell)`.
- [ ] Anotar versión exacta (cambia el camino de captura):
  ```
  adb shell getprop ro.build.version.release   # confirmado: 16
  adb shell getprop ro.build.version.sdk        # confirmado: 36
  adb shell getprop ro.build.version.oneui      # confirmado: 80500 (One UI 8)
  ```

## Spike 1 — Captura saca 1 frame a PNG (el más importante) — ✅ PASÓ

**Resultado (2026-06-17, S24 Ultra Android 16/One UI 8)**: PASÓ. El frame PNG
(~3 MB) muestra la pantalla real del teléfono, confirmado visualmente. Captura
vía `DisplayManager.createVirtualDisplay` estática (espejo por default), sin
`Workarounds`, como uid shell. Riesgos R1/R2/R3/R7 despejados en device. Detalle
del camino confirmado en ADR-0002.

**Objetivo**: confirmar que el display produce píxeles. Sin MediaCodec, sin red,
sin Rust. Aísla "¿la captura funciona?" de todo lo demás.

- Mini-jar Kotlin corrido por `app_process` que, por reflexión:
  1. `Workarounds.fillConfigurationController()` (port de scrcpy) — **obligatorio
     en Samsung**, si no NPE en `getDisplayInfoLocked`.
  2. Resuelve resolución con `DisplayManagerGlobal.getDisplayInfo(0)`.
  3. Crea un `ImageReader` (NO MediaCodec) y obtiene su `Surface`.
  4. `DisplayManager.createVirtualDisplay(name, w, h, displayIdToMirror=0,
     surface)` por reflexión **estática** (camino correcto Android 16, validado).
  5. Lee 1 `Image` y la vuelca a PNG en `/data/local/tmp/frame.png`.
  6. Ante `NoSuchMethodException`: enumerar `getDeclaredMethods()` y logear con
     `Build.MANUFACTURER/MODEL` y `SDK_INT` (Samsung cambia firmas).
- `adb pull /data/local/tmp/frame.png` y abrir el PNG en la PC.

**Lectura del resultado:**
| Resultado | Significa | Acción |
|-----------|-----------|--------|
| PNG con contenido | Captura OK | Avanzar a Spike 2 |
| PNG negro, proceso OK | FLAG_SECURE / One UI black-screen (problema de **captura**, no de encode) | Investigar flags del display; probar `--new-display` |
| Crash en `createVirtualDisplay` | Camino equivocado o firma cambiada por Samsung | Revisar el log de métodos enumerados |

## Spike 2 — Encode a archivo local — ✅ PASÓ

**Resultado (2026-06-17, S24 Ultra)**: PASÓ. El encoder H.265 por hardware
`c2.qti.hevc.encoder` produjo un elementary stream HEVC válido (44.717 bytes)
desde el mismo virtual display del Spike 1. R8 (encoder negro) despejado. El
conteo de ~1 frame en 3 s es esperado: un mirror solo emite frames nuevos cuando
la pantalla cambia; con la pantalla estática solo sale el keyframe inicial. Con
movimiento, los frames fluyen.

**Objetivo**: aislar el encoder (MediaCodec puede dar negro aun con captura OK
en Samsung 15). Solo tras un PNG con contenido.

- Conectar la `Surface` de un `MediaCodec` (H.265 o H.264 por hardware) como
  destino del virtual display, en vez del `ImageReader`.
- Volcar los NAL units a un archivo `/data/local/tmp/out.h265`.
- `adb pull` y abrir con `ffplay out.h265` en la PC.
- Si reproduce → encoder OK. Si negro → problema de encoder (probar otro códec /
  sw encoder), no de captura.

## Spike 3 — Sockets sobre el túnel ADB + cliente Rust — ✅ PASÓ

**Resultado (2026-06-17, S24 Ultra)**: PASÓ. El cliente Rust (`transport-spike`,
que ejercita `skry-adb` + `skry-proto`) lanzó el server (`Spike3Main`) vía
app_process, hizo el forward, recibió el handshake (`SM-S928B 1440x3120 codec=h265`)
y **949 frames / ~10 MB** de H.265 por el túnel ADB en 10 s. Paridad de wire
Java↔Rust confirmada end-to-end en device. Todo el transporte del MVP validado.

**Objetivo**: recién acá entran la red y el wire ya testeado. Solo tras Spike 2.

- El server escucha en `localabstract:skry`, emite el handshake + frames por el
  canal de video (protocolo de `server/protocol`, ya implementado).
- El cliente Rust (`skry-adb` para el túnel + `skry-proto` para el wire) hace
  `forward`, conecta los dos sockets con su byte de tipo, lee el handshake y los
  frames, y los vuelca/decodifica.
- Primero por **USB** (descarta R4: el wireless debugging del Samsung se apaga en
  idle). Una vez estable, repetir por **Wi-Fi** (`adb pair`/`connect`, ya en
  `skry-adb`) para validar la operación inalámbrica end-to-end y medir latencia
  glass-to-glass.

## Qué es testeable sin dispositivo, y qué no

- **Sin device (CI/Docker)**: protocolo (ambos lados), parseo de adb, selección
  de marcha, lógica de selección de API por `SDK_INT` (con dobles), parseo de
  args de invocación.
- **Requiere el S24**: la reflexión real contra el framework, la captura, el
  encode, y la latencia. Esto lo valida el usuario; el código se diseña detrás de
  interfaces para ejercitar la lógica con dobles y dejar solo la integración al
  device.
