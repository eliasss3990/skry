# Build desde el código

El toolchain de build está **dockerizado** para ser reproducible e idéntico a
CI (ver [ADR-0004](decisions/0004-build-docker-runtime-host.md)). El **runtime**
de `skry` corre nativo en tu host (necesita ADB, GPU y ventana).

## Requisitos

- Docker.
- Para *correr* `skry` (no para buildear): `adb` y los drivers de tu GPU.

## Cliente (Rust)

Todo se invoca a través del wrapper `scripts/dev`, que construye la imagen de
build la primera vez, mapea tu usuario (no deja archivos root-owned) y cachea
las dependencias de cargo en `client/.cargo-home/` (gitignored).

```bash
scripts/dev cargo test                                   # tests
scripts/dev cargo clippy --all-targets -- -D warnings    # lint
scripts/dev cargo fmt --check                            # formato
scripts/dev cargo build --release                        # binario (Linux)
scripts/dev bash                                         # shell en la imagen
```

La imagen incluye las libs nativas (FFmpeg, SDL2) que requiere el crate de
video, además de `clippy` y `rustfmt`.

Todas las imágenes corren como **usuario no-root** (ADR-0006). `scripts/dev`
además mapea tu uid para que los artefactos no queden root-owned.

### Artefacto de release (Linux, multi-stage)

`build/release.Dockerfile` es multi-stage: compila en un stage pesado y deja una
imagen final limpia (sólo libs de runtime + binario, sin toolchain ni fuentes),
que corre como usuario sin privilegios. Extraer sólo el binario:

```bash
docker build -f build/release.Dockerfile --target export --output type=local,dest=dist .
# -> dist/skry
```

(Requiere que el `.jar` del server esté embebido; lo coloca la CI de release.)

### Binario de Windows

No se cross-compila desde Linux (FFmpeg/SDL2 nativas lo hacen frágil). El binario
de Windows se produce en CI sobre un runner `windows-latest`. Para buildear
localmente en Windows, instalar Rust 1.83 y las libs nativas con vcpkg (ver
`.github/workflows/`).

## Server (Android, Kotlin)

Documentado junto al código del server en `server/` cuando esté disponible
(imagen de build propia con Android SDK + Gradle).

## CI

`.github/workflows/ci.yml` corre, en cada push y PR:

- **Linux**: dentro de la misma imagen Docker (`build/client.Dockerfile`) →
  paridad exacta con el desarrollo local. Formato + clippy + tests.
- **Windows**: build nativo de los crates sin dependencias nativas (se expande a
  medida que se integran FFmpeg/SDL2 vía vcpkg).
