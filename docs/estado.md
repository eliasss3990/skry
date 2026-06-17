# Estado del proyecto — handoff 2026-06-17

Resumen vivo de qué está hecho, qué está validado y qué sigue. Para el detalle
de diseño ver [architecture.md](architecture.md) y los [ADRs](decisions/).

## Validado en hardware real (S24 Ultra, Android 16 / One UI 8)

Los tres spikes pasaron en el dispositivo del usuario (ver [spikes.md](spikes.md)):

1. **Captura** de pantalla via `DisplayManager.createVirtualDisplay` (hidden API,
   uid shell) → PNG con la pantalla real.
2. **Encode** H.265 por hardware (`c2.qti.hevc.encoder`).
3. **Transporte** end-to-end: server por `LocalServerSocket` + cliente Rust por
   túnel ADB, con **paridad de wire Java↔Rust** confirmada (949 frames / ~10 MB).

Conclusión: **toda la cadena del MVP está probada**. Lo que sigue es ingeniería
conocida sobre cimientos validados.

## Implementado y en CI verde (Linux)

- `skry-proto` (Rust) y `server/protocol` (Kotlin): protocolo de wire, paridad
  byte a byte testeada en ambos lados.
- `skry-adb`: wrapper de adb con descubrimiento, resiliencia, y connect/pair/mDNS
  inalámbrico.
- `skry-video`: decode (FFmpeg software) + render (SDL2) + `PresentationClock`
  (presentación por `pts`).
- `skry`: binario orquestador (CLI). Compila en Linux; junta adb+proto+video con
  lazo decode/render en hilos.
- `transport-spike`: cliente de validación (cross-compila a Windows; el usuario lo
  corrió). Tiene modo `--pipe` para ver el video con ffplay.
- Infra: build dockerizado no-root, 3 jobs de CI (cliente Linux, cliente Windows
  parcial, server Kotlin), spikes Android (build+dex).

## Ver el video AHORA (provisional, ya funciona)

En **cmd** de Windows (el jar ya está en el teléfono de las pruebas):

```cmd
transport-spike.exe --pipe | ffplay -fflags nobuffer -flags low_delay -framerate 120 -f hevc -i -
```

Limitación: ffplay no usa el `pts` → el ritmo no es exacto. El cliente `skry`
(FFmpeg/SDL2) lo resuelve presentando por `pts`.

## Lo que falta (en orden de valor)

1. **Build de Windows del cliente `skry`** (FFmpeg vía vcpkg + SDL2 bundled). Es
   el bloqueante para que el usuario corra el `skry` real y vea el video sin
   ffplay. Plan completo en [ADR-0007](decisions/0007-build-windows-vcpkg.md).
   Implica alinear FFmpeg a la misma major en Linux y Windows — hacerlo con
   validación para no romper el Linux verde. **Próximo paso a la mañana.**
2. **Módulo `:app` Android productivo** (reemplaza los spikes, con capa
   `ScreenCapture` por `SDK_INT` multi-dispositivo) + **embeber el jar** en el
   binario (`include_bytes!`) + **push automático** (hoy el jar se empuja a mano).
3. **Cablear el canal de control + sistema de marchas** (el protocolo ya lo
   soporta; el orquestador hoy sólo abre el canal de video).
4. **hw decode** (DXVA2/D3D11VA en Windows) para tiempo real a 1440×3120@120
   (hoy el decode es software → va a ir lento a full res).
5. **Release CI** que publique los binarios (los install scripts ya están, pero
   no hay release todavía).

## Pendientes menores (de reviews, no bloquean)

- `Box::leak` del TextureCreator en el renderer: aceptable single-use, documentar
  como deuda si `Renderer` se recrea.
- Deduplicar `connect_and_handshake`/`forward_child_output` entre `skry` y
  `transport-spike` (a `skry-adb` o un `skry-transport`).
- Caché de CI (Docker layers + Gradle) para acelerar el pipeline.
