# Arquitectura de skry

`skry` (de *scry*, "ver a distancia") es una herramienta de línea de comandos
para **espejar la pantalla de un teléfono Android en la PC con baja latencia**.
El teléfono captura y codifica su pantalla; la PC recibe, decodifica y la pinta
en una ventana. Es conceptualmente de la misma familia que `scrcpy`, y reusa
varias de sus ideas probadas, con una CLI propia, un sistema de "marchas" de
fluidez y una estrategia de transporte extensible.

> Alcance: espejado del **dispositivo propio del usuario**, autorizado vía
> depuración de Android (ADB). No es una herramienta de acceso remoto a
> dispositivos ajenos: requiere autorización explícita del dueño del teléfono en
> la pantalla del propio dispositivo.
>
> Funciona **de forma inalámbrica** (ADB sobre Wi-Fi, teléfono a unos metros de
> la PC, sin cable) y es **100% local**: el video viaja sólo por el enlace local
> entre teléfono y PC, sin pasar por internet ni consumir datos móviles. Ver
> [ADR-0005](decisions/0005-inalambrico-y-local.md).

## Panorama

```
┌─────────────────────────── PC (cliente, Rust) ───────────────────────────┐
│                                                                           │
│  CLI (clap)                                                               │
│     │                                                                     │
│     ▼                                                                     │
│  Orquestador ──► skry-adb ──► (adb push jar, app_process, forward/reverse)│
│     │                                                                     │
│     ├──► Canal de control (TCP)  ◄── handshake, telemetría, marchas       │
│     │                                                                     │
│     └──► Canal de video (TCP sobre túnel ADB)                             │
│                │                                                          │
│                ▼                                                          │
│           Decoder (FFmpeg: hwaccel con fallback a CPU)                    │
│                │                                                          │
│                ▼                                                          │
│           Renderer (SDL2, gestión de VSync)                              │
└───────────────────────────────────────────────────────────────────────────┘
                              ▲                │
                  control TCP │                │ video
                              │                ▼
┌─────────────────────── Teléfono (server, Kotlin .jar) ────────────────────┐
│                                                                           │
│  app_process (uid shell, sin app instalada, efímero en /data/local/tmp)   │
│     │                                                                     │
│     ├──► Captura: SurfaceControl / DisplayManager (hidden APIs)           │
│     │                                                                     │
│     ├──► Encoder: MediaCodec (H.265 / H.264, hardware)                    │
│     │         ▲                                                           │
│     │         └── ajuste de bitrate en caliente (Bundle) ← marchas        │
│     │                                                                     │
│     └──► Canales TCP (control + video) sobre el túnel ADB                 │
└───────────────────────────────────────────────────────────────────────────┘
```

## Decisiones de diseño clave

Las decisiones con peso arquitectónico viven como ADRs en `docs/decisions/`.
Resumen:

| # | Decisión | Estado |
|---|----------|--------|
| [0001](decisions/0001-transporte-adb-first.md) | Transporte sobre túnel ADB como base; Wi-Fi Direct como capa opcional posterior | Aceptada |
| [0002](decisions/0002-captura-surfacecontrol.md) | Captura via SurfaceControl/DisplayManager (no MediaProjection) por correr como uid shell | Aceptada |
| [0003](decisions/0003-defaults-conservadores.md) | Defaults conservadores (tasa nativa/60 FPS); 120/144 como opt-in | Aceptada |
| [0004](decisions/0004-build-docker-runtime-host.md) | Build dockerizado y reproducible; runtime nativo en el host | Aceptada |
| [0005](decisions/0005-inalambrico-y-local.md) | Operación inalámbrica (ADB sobre Wi-Fi) y 100% local, sin internet ni datos móviles | Aceptada |
| [0006](decisions/0006-minimo-privilegio.md) | Mínimo privilegio (contenedores no-root, server como uid shell) y artefactos limpios (multi-stage) | Aceptada |

## Estado de implementación

Este documento describe el diseño completo; no todo está construido aún. Estado
por componente:

| Componente | Estado |
|------------|--------|
| `skry-proto` (protocolo) | Implementado y testeado |
| `skry-adb` (wrapper adb) | Implementado y testeado (falta `connect`/`pair`/mDNS para el flujo inalámbrico completo) |
| `skry` (binario orquestador) | Pendiente |
| `skry-transport` | Pendiente |
| `skry-video` (decode/render) | Pendiente |
| Server Android (`server/`) | Pendiente |
| Install scripts / release CI | Pendiente |

## Componentes

### Cliente (Rust)

Workspace Cargo en `client/`. Crates:

- **`skry`** — binario CLI. Parseo de flags (clap), orquestación del ciclo de
  vida (descubrir dispositivo → desplegar server → handshake → stream → cierre
  con gracia), y el lazo de control de marchas.
- **`skry-proto`** — definición del protocolo (mensajes de handshake, control y
  framing de video) con su (de)serialización. Sin dependencias de I/O: es
  lógica pura y 100% testeable. Es el contrato compartido con el server.
- **`skry-adb`** — wrapper tipado sobre el binario `adb`. Encapsula
  `get-state`, parseo de `devices`, `push`, `forward`/`reverse` y el spawn via
  `app_process`. Acá vive la resiliencia de conexión física (sin dispositivo,
  múltiples, no autorizado).
- **`skry-transport`** — abstracción del transporte (hoy: túnel ADB sobre TCP;
  mañana: Wi-Fi Direct / LAN). El resto del cliente no conoce el medio físico.
- **`skry-video`** — decodificación (FFmpeg) y render (SDL2). Aislado para que
  la lógica de orquestación no dependa de las libs nativas pesadas.

Asincronismo con `tokio`. La separación en crates mantiene lo testeable sin
hardware (proto, adb-parsing, transport) lejos de lo que exige GPU/ventana/USB.

### Server (Android, Kotlin)

Proyecto Gradle en `server/`. Produce `skry-server.jar`, que el cliente
**embebe** (`include_bytes!`) y empuja a `/data/local/tmp/` para correrlo con
`app_process`. No se instala ninguna APK: el proceso vive mientras dura la
sesión y muere al cerrarse el cliente. Sin rastros persistentes — no porque
"evada detección", sino porque no deja una app instalada en el teléfono.

Responsabilidades: capturar el framebuffer (hidden APIs de SurfaceControl/
DisplayManager), codificar con MediaCodec según lo negociado en el handshake, y
exponer los canales de control y video sobre el túnel ADB.

### Protocolo

Documentado en [protocol.md](protocol.md). Versionado explícito desde el primer
byte del handshake para poder evolucionar cliente y server de forma
independiente sin romper compatibilidad silenciosamente.

## El sistema de "marchas"

Tres marchas atadas a tasas de refresco objetivo: **144, 120 y 60 FPS**. El
canal de control monitorea latencia y frames perdidos; ante inestabilidad
(red saturada, throttling térmico del teléfono) el cliente ordena un *downgrade*
y el server ajusta bitrate/framerate **en caliente** vía `Bundle`, sin cortar el
flujo. La marcha de arranque es conservadora (ver ADR-0003): se sube sólo si el
dispositivo y la red lo sostienen.

## Resiliencia

Principio rector: **el programa nunca explota en silencio. Si puede
auto-recuperarse, lo hace; si no, le dice al usuario exactamente qué hacer.**
El catálogo de casos borde (errores de ADB, de red, de hardware de decodificación
y de runtime) está en [resilience.md](resilience.md).

## Qué se puede validar sin hardware, y qué no

Este proyecto se construye sin un teléfono conectado al entorno de desarrollo.
Por eso la arquitectura aísla deliberadamente lo testeable de lo que no:

- **Validable en CI/Docker/Linux**: protocolo (round-trips de serialización),
  parseo de salidas de `adb`, lógica de selección de marchas, máquinas de
  estado de conexión y reconexión, CLI parsing.
- **Requiere dispositivo físico** (lo valida el usuario en su hardware): el
  stream de punta a punta, la captura real en Android, el decode por GPU y el
  render SDL2 en la ventana de Windows.

Los componentes del segundo grupo se diseñan detrás de interfaces para poder
ejercitar su lógica con dobles de prueba, dejando sólo la integración final
para la validación manual.
