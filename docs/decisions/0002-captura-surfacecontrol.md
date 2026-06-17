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

Capturar el framebuffer usando las **APIs ocultas del framework**
(`android.view.SurfaceControl`, `android.hardware.display.DisplayManagerGlobal`,
`IDisplayManager`), accedidas por reflexión. Es el mismo mecanismo que usa
`scrcpy`: como `app_process` corre con uid `shell`, tiene permiso para crear un
*virtual display* / leer el display físico sin diálogo de consentimiento.

El pipeline: crear una `Surface` de entrada del encoder `MediaCodec`, conectarla
como destino de un virtual display espejo del display físico vía
`SurfaceControl.createDisplay()` + `setDisplaySurface()`, y leer los buffers
codificados de salida.

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
