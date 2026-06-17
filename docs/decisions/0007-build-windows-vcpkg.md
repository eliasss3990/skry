# ADR 0007: Build de Windows del cliente (FFmpeg vía vcpkg, SDL2 bundled)

- Estado: Propuesta (plan para implementar con validación)
- Fecha: 2026-06-17

## Contexto

El binario `skry` enlaza FFmpeg (decode) y SDL2 (render) a través de los crates
`ffmpeg-next` y `sdl2`. Per [ADR-0004](0004-build-docker-runtime-host.md), el
binario de Windows se compila **nativo en CI** (runner `windows-latest`), no por
cross-compile. Falta implementar ese job.

El obstáculo central es la **alineación de versiones de FFmpeg** entre plataformas:
- Linux (Debian bookworm en `build/client.Dockerfile`) trae FFmpeg **5.1** →
  hoy `ffmpeg-next = "5.1.1"`.
- vcpkg mainline en `windows-latest` provee FFmpeg **7.x**. Pinear vcpkg a 5.1
  requiere un baseline viejo que arrastra TODO el registry a 2023 (SDL2 incluido)
  sin parches — no conviene.

`ffmpeg-next` debe matchear la major de FFmpeg en cada plataforma, pero es un
único `Cargo.toml`: **ambas plataformas deben usar la misma major de FFmpeg.**

## Decisión

Alinear **ambas plataformas a FFmpeg 7.x** y `ffmpeg-next = "7"`:

- **Windows**: FFmpeg 7 vía **vcpkg**, triplet **`x64-windows-static`** (binario
  único sin DLLs al lado). SDL2 **no** por vcpkg: usar el crate `sdl2` con
  features **`bundled` + `static-link`** (compila SDL2 desde fuente con
  cmake/MSVC, ya presentes en el runner). Así vcpkg sólo provee FFmpeg.
- **Linux** (build image): subir la base a una con FFmpeg 7 (Debian trixie o
  compilar FFmpeg 7), de modo que `ffmpeg-next = "7"` compile también ahí.

### Cambios concretos

1. `client/crates/skry-video/Cargo.toml`:
   ```toml
   ffmpeg-next = "7"
   sdl2 = { version = "0.37", features = ["bundled", "static-link"] }
   ```
2. `client/vcpkg.json` (nuevo, modo manifest):
   ```json
   { "name": "skry-client", "version": "0.1.0",
     "dependencies": [ { "name": "ffmpeg",
       "features": ["avcodec","avformat","avutil","swscale","swresample"] } ] }
   ```
3. `.github/workflows/ci.yml`, job `client-windows`: bootstrap vcpkg, cache de
   `C:\vcpkg\installed` (key por hash de `vcpkg.json`), `vcpkg install
   ffmpeg:x64-windows-static`, y `cargo {fmt,clippy,test,build --release -p skry}`
   con `FFMPEG_DIR=C:\vcpkg\installed\x64-windows-static` y
   `LIBCLANG_PATH=C:\Program Files\LLVM\bin` (bindgen de ffmpeg-next).
4. `build/client.Dockerfile`: base con FFmpeg 7 (validar que el Linux verde no se
   rompa ANTES de pushear — rebuild local de la imagen + `cargo build`).

### Tiempos

Primer build de vcpkg-FFmpeg: ~25-40 min; con `actions/cache` baja a ~5 min.

## Por qué no se hizo ya

Toca tres frentes a la vez (Cargo.toml, base de Linux, job de Windows) y el
resultado final —un `.exe` que decodifica y rendererea— sólo se valida corriéndolo
en una PC Windows con un teléfono. Hacerlo a ciegas arriesga romper el Linux verde
o entregar un binario que no anda. Se implementa con validación: rebuild local de
la imagen Linux para no romper CI, y prueba del `.exe` en el host del usuario.

## Consecuencias

- **Positivas**: un único `.exe` portable (static), versiones de FFmpeg alineadas
  cross-platform, build reproducible en CI.
- **Negativas**: subir la base de Linux a FFmpeg 7 es un cambio de imagen con su
  propia verificación; el primer build de vcpkg es lento (mitigado con cache).
