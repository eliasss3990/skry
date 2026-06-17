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

### Ruta por versión de Android (importante)

La API concreta para crear el virtual display **cambió y se cerró con las
versiones**, y el dispositivo objetivo (Samsung S24 Ultra, Android 14/15) está
en el extremo nuevo:

- **`SurfaceControl.createDisplay(String, boolean)` fue REMOVIDO en Android 14.**
  No usar esa firma como camino de arranque: no existe en el Android del S24.
- Android 14+ requiere la ruta basada en `DisplayManagerGlobal` /
  `IDisplayManager` (creación de virtual display por la vía interna no-
  `MediaProjection`), que volvió a cambiar entre 14 y 15.

Decisión operativa: **portar la estrategia de captura de la versión actual de
`scrcpy` (2.x/3.x)**, que ya resolvió Android 11→15, en vez de implementar desde
cero la ruta legacy. La implementación de arranque apunta a **Android 14+**;
las versiones viejas son, si acaso, fallbacks posteriores. Todo el acceso por
reflexión se aísla en una capa fina con selección por nivel de API y mensajes de
error claros si una firma no está.

> Este es uno de los riesgos altos del proyecto (R3 del pre-mortem): la captura
> es el corazón del server y el único dispositivo de validación es de los más
> nuevos + OEM Samsung. Validar **sólo la captura** (sacar 1 frame) en el S24
> antes de cablear encode/red.

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
