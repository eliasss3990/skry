# ADR 0002: Captura via SurfaceControl/DisplayManager, no MediaProjection

- Estado: Aceptada
- Fecha: 2026-06-16

## Contexto

El plan proponía capturar la pantalla con `MediaProjection`. Pero
`MediaProjection` requiere:

- Un **contexto de aplicación** (Activity/Service) para lanzar el intent de
  consentimiento (`createScreenCaptureIntent`).
- Un **token de consentimiento** que el usuario otorga mediante un diálogo del
  sistema.

El server de skry **no es una app**: corre como uid `shell` vía `app_process`,
sin Activity, sin Context de app y sin posibilidad de mostrar el diálogo de
consentimiento. Por lo tanto **`MediaProjection` no es usable en este modelo de
ejecución**.

## Decisión

Capturar el framebuffer usando las **APIs ocultas del framework**, accedidas por
reflexión. Es el mismo mecanismo que usa `scrcpy`: como `app_process` corre con
uid `shell`, tiene permiso para crear un *virtual display* / leer el display
físico sin diálogo de consentimiento.

El pipeline: crear una `Surface` de entrada del encoder `MediaCodec` y
conectarla como destino de un virtual display espejo del display físico.

### Ruta por versión de Android (hechos verificados)

La API concreta para crear el virtual display cambió con las versiones. Hechos
verificados contra el código y los issues actuales de `scrcpy` (no asumir):

- **Camino primario (espejo del display físico)**: reflexión sobre el método
  **estático** `android.hardware.display.DisplayManager.createVirtualDisplay(
  String name, int width, int height, int displayIdToMirror, Surface surface)`
  con `displayIdToMirror = 0`. **No** es `DisplayManagerGlobal` ni
  `IDisplayManager` (ésos se usan sólo para `getDisplayInfo`/`getDisplayIds`).
- **Camino "display nuevo"**: constructor privado `DisplayManager(Context)` con
  un `FakeContext`, y `createVirtualDisplay(...)` con flags ocultos
  (`VIRTUAL_DISPLAY_FLAG_OWN_FOCUS`, `FLAG_DEVICE_DISPLAY_GROUP` en API 34+).
- **`SurfaceControl.createDisplay(String, boolean)` es sólo fallback** para
  Android ≤ 14. Fue **REMOVIDO en Android 15 / One UI 7** (no en 14). Si el S24
  ya está en Android 15/One UI 7, ese método no existe.
- **Orden de intentos**: `DisplayManager.createVirtualDisplay` →
  (fallback) `SurfaceControl.createDisplay`. No al revés.

### Prerequisitos y trampas en Samsung (S24 Ultra)

- **`Workarounds.fillConfigurationController()` (port de scrcpy) es OBLIGATORIO
  en Samsung**, no opcional: sin él, `DisplayManagerGlobal.getDisplayInfoLocked()`
  tira NPE desde Android 12. Es prerequisito para que arranque en el device de
  validación.
- **Samsung modifica firmas de métodos internos** del framework: ante
  `NoSuchMethodException`, la capa de reflexión debe **enumerar
  `getDeclaredMethods()` y logearlos**, junto con `Build.MANUFACTURER/MODEL` y
  `VERSION.SDK_INT`. "Mensajes claros" significa concretamente esto.
- **Pantalla negra con resolución correcta**: si cualquier surface del display
  tiene `FLAG_SECURE` (Samsung Pay/Pass, Secure Folder), o por bugs de One UI, el
  frame sale negro aunque la captura "funcione". Es un modo de falla de captura,
  no de encode.

Decisión operativa: **portar la estrategia de captura de `scrcpy` (2.x/3.x)**, que
ya resolvió Android 11→15, con el camino de arranque correcto de arriba. Aislar
todo el acceso por reflexión tras una interfaz `ScreenCapture` con selección por
`SDK_INT`, testeable con dobles; la integración real se valida en el device.

### VALIDADO en el device (Spike 1, Android 16 / One UI 8 / API 36)

Corrida real en el S24 Ultra (`SM-S928B`), 2026-06-17. Hechos confirmados:

- **El camino que funciona** es `DisplayManager.createVirtualDisplay(String name,
  int width, int height, int densityDpi, Surface surface)` — la sobrecarga
  **estática**. Existe en API 36 y creó el virtual display (id 33, espejo del
  físico 1440×3120) sin crash.
- Un virtual display **sin** `VIRTUAL_DISPLAY_FLAG_OWN_CONTENT_ONLY` **espeja la
  pantalla principal por default**: por eso captura el contenido real sin pasar
  un `displayIdToMirror` explícito.
- **`SurfaceControl.createDisplay` / `createVirtualDisplay` NO existen** en esta
  versión (confirmada la remoción).
- **`Workarounds.fillConfigurationController()` NO fue necesario** en Android 16:
  la captura funcionó como uid shell sin el fake ActivityThread. (En 12-14 podía
  hacer falta; en 16 no.)
- `DisplayManagerGlobal.getInstance().getDisplayInfo(0)` da el tamaño lógico
  (`logicalWidth`/`logicalHeight`).
- Captura a PNG vía `ImageReader`: frame de ~3 MB (contenido real, no negro).

Esto es el camino de arranque confirmado para el módulo `:app`; los demás quedan
como fallbacks para versiones más viejas.

### Soporte multi-dispositivo (requisito explícito)

skry debe andar en **distintos teléfonos**, no solo en el de validación. Esto se
logra, igual que scrcpy (un único jar para Android 5→16), por estas decisiones:

- **Reflexión, no enlace en compilación**, para todo lo hidden: el comportamiento
  se resuelve en runtime contra el framework del dispositivo. Compilar contra
  `android.jar` de API 36 **no** ata el runtime a API 36.
- `minSdk` bajo + uso directo sólo de APIs estables (`ImageReader`, `MediaCodec`,
  `Surface`, `Bitmap`); cualquier API nueva se llama por reflexión con guarda por
  `SDK_INT`.
- **Capa `ScreenCapture` con selección por `Build.VERSION.SDK_INT`**, con una
  implementación por familia de versiones:
  - Android 15/16: `DisplayManager.createVirtualDisplay` estática (validado).
  - Android ≤14: `SurfaceControl.createDisplay` (presente en esas versiones).
  - `densityDpi` real del display (no 0 como en el spike).
- **Enumeración de métodos ante `NoSuchMethodException`** para adaptarse a OEMs
  que cambian firmas (prototipado en el Spike 1).
- Matriz de referencia: el código known-good de scrcpy por versión.

**Límite honesto**: la *validación* sólo es posible en los dispositivos físicos
disponibles. El S24 Ultra (Android 16/One UI 8) cubre el caso más hostil; otras
versiones se cubren por la matriz de scrcpy y se validan cuando haya hardware.

> Riesgo alto (R1-R3 del pre-mortem): la captura es el corazón del server y el
> único device de validación (Samsung + One UI + Android 14/15) es de los más
> hostiles para esta técnica — scrcpy tiene issues **abiertos sin fix** en este
> hardware. Por eso el primer spike es **ImageReader → PNG por USB** (sin
> MediaCodec, sin red): aísla "¿el display produce píxeles?" de todo lo demás.
> Detalle del plan de spikes en la tarea correspondiente.

## Consecuencias

- **Positivas**: funciona en el modelo `app_process`/shell sin UI; sin diálogos
  de permiso en cada arranque; coherente con el enfoque efímero.
- **Negativas**: las hidden APIs **no son estables entre versiones de Android**.
  Mitigación: aislar todo el acceso por reflexión en una capa fina con
  *fallbacks* por versión y mensajes de error claros si una firma cambió;
  documentar las versiones de Android probadas. La validación real corre sobre
  el dispositivo del usuario (Samsung S24 Ultra, Android moderno).
- **Negativas**: requiere mantener compatibilidad con cambios de las APIs
  internas a futuro. Se asume como costo inherente a esta categoría de
  herramienta (lo mismo aplica a scrcpy).
