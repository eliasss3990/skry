# ADR 0007: Build de Windows del cliente (FFmpeg vía vcpkg, SDL2 bundled)

- Estado: Aceptada (implementada y verde en CI, 2026-06-18)
- Fecha: 2026-06-17

## Contexto

El binario `skry` enlaza FFmpeg (decode) y SDL2 (render) vía los crates
`ffmpeg-next` y `sdl2`. Per [ADR-0004](0004-build-docker-runtime-host.md), el
binario de Windows se compila **nativo en CI** (`windows-latest`), no por
cross-compile.

El obstáculo que se temía era la **alineación de versiones de FFmpeg** (Linux
tiene 5.1 de apt; vcpkg da 8.x). Resultó **más simple de lo previsto**: la crate
`ffmpeg-next 8.x` compila contra un rango amplio de FFmpeg (4→8) detectando
features por versión, así que **no hubo que tocar la base de Linux** — Linux
sigue usando el FFmpeg 5.1 del sistema y Windows el 8.x de vcpkg, con la misma
crate.

## Decisión (cómo quedó implementado)

- **`ffmpeg-next = "8.1"`** en `skry-video`, target-specific:
  - `cfg(windows)`: con feature **`static`** (hace que `ffmpeg-sys` busque el
    triplet estático de vcpkg, no el dinámico).
  - `cfg(not(windows))`: sin features (FFmpeg dinámico del sistema).
- **FFmpeg en Windows**: vcpkg, triplet **`x64-windows-static-md`** (libs
  estáticas, CRT dinámico — combina con el linkeo MSVC por defecto de Rust;
  mezclar CRTs distintos es fuente de bugs, por eso `-md` y no `-static`).
  `vcpkg.json` con `avcodec,avformat,swscale,swresample` (sin `avutil`, que es
  core). Instalado en **modo clásico** (`C:\vcpkg\installed`) para que la crate
  `vcpkg` de `ffmpeg-sys` lo descubra.
- **SDL2 en Windows**: crate `sdl2` con **`bundled` + `static-link`** (compila
  SDL desde fuente, estático) → un solo `.exe` sin DLLs al lado.
- **Libs de sistema** (`skry-video/build.rs`, solo en Windows): el FFmpeg
  estático necesita `strmiids` (DirectShow/avdevice), `ncrypt`/`crypt32`/
  `secur32` (schannel-TLS de avformat), `shlwapi`, `mfplat`/`mfuuid`, etc. — la
  discovery de vcpkg no las propaga al linker.

### Variables de entorno del job (`windows-bin`)

- `VCPKGRS_TRIPLET=x64-windows-static-md` (linkeo vía la crate vcpkg).
- `BINDGEN_EXTRA_CLANG_ARGS=-IC:/vcpkg/installed/.../include` (bindgen no recibe
  el include por la discovery; se lo pasamos explícito).
- `LIBCLANG_PATH=C:\Program Files\LLVM\bin` (bindgen).
- `CMAKE_POLICY_VERSION_MINIMUM=3.5` (CMake 4.x del runner rechaza el
  `cmake_minimum_required` viejo del SDL2 bundled).

### Estructura de CI

Dos jobs: `windows-ffmpeg` (instala FFmpeg vía vcpkg y lo **cachea** — es lo
lento, ~25-40 min la primera vez) y `windows-bin` (`needs` el anterior, restaura
el cache, compila `skry.exe` y lo sube como artefacto). El split evita rebuildear
FFmpeg en cada iteración.

## Escollos que aparecieron (lecciones)

1. **CMake 4.x** removió compatibilidad con `cmake_minimum_required < 3.5` → el
   SDL2 bundled no configuraba. Fix: `CMAKE_POLICY_VERSION_MINIMUM=3.5`.
2. **`avfft.h` removido en FFmpeg 8** → `ffmpeg-sys 7.x` lo incluía sí o sí y no
   lo encontraba (fallback a `/usr/include`). Fix: subir a `ffmpeg-next 8.1`.
3. **`VCPKGRS_DYNAMIC`**: sin el feature `static`, `ffmpeg-sys` busca el triplet
   dinámico (`x64-windows`) en vez del estático instalado. Fix: feature `static`.
4. **~55 unresolved externals** de libs de sistema del FFmpeg estático. Fix:
   `build.rs` que las enlaza.

## Pendiente / deuda

- **Pinear el commit de vcpkg** (hoy flota con el runner → la versión de FFmpeg
  podría cambiar sin que el cache key —hash de `vcpkg.json`— lo note). Riesgo de
  build "verde" con otra versión de FFmpeg.
- **Job de release** que publique el `.exe` (y el binario Linux) en los tags: los
  install scripts apuntan a releases que aún no se generan.

## Consecuencias

- **Positivas**: un único `.exe` portable (estático), misma crate en ambas
  plataformas sin tocar la base de Linux, build reproducible y cacheado en CI.
- **Negativas**: vcpkg sin pinear (ver deuda); el primer build de FFmpeg es lento
  (mitigado con cache).
