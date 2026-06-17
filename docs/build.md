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

No se cross-compila desde Linux (FFmpeg/SDL2 nativas lo hacen frágil): se compila
nativo en CI sobre `windows-latest` con FFmpeg vía vcpkg y SDL2 bundled. El plan
concreto (triplet, vcpkg.json, env del job, alineación de versiones de FFmpeg
entre Linux y Windows) está en
[ADR-0007](decisions/0007-build-windows-vcpkg.md). Pendiente de implementar con
validación para no romper el build Linux.

## Server (Android)

Dos partes:

- **`server/protocol`** (Kotlin/JVM puro, sin Android SDK): el wire del protocolo.
  Se compila/testea con el wrapper de Gradle:
  ```bash
  cd server && ./gradlew :protocol:test
  ```
- **`server/spike`** (Java, spikes de validación corridos en device): se compila
  y dexea en un `.jar` con la imagen del SDK de Android (`build/android.Dockerfile`,
  no-root) y `server/spike/build-spike.sh`:
  ```bash
  docker build -t skry-build-android:local -f build/android.Dockerfile .
  docker run --rm --user "$(id -u):$(id -g)" -v "$PWD":/work -w /work \
      skry-build-android:local bash server/spike/build-spike.sh
  # -> dist/skry-spike.jar
  ```

El módulo `:app` productivo (que reemplaza los spikes, con la capa
`ScreenCapture` por `SDK_INT`) está pendiente.

### Ver el video de forma provisoria (sin el cliente final)

El binario `transport-spike` (cliente Rust de validación) puede pipear el stream
crudo a `ffplay` para ver el teléfono en vivo mientras no esté el render propio:

```cmd
transport-spike.exe --pipe | ffplay -fflags nobuffer -flags low_delay -framerate 120 -f hevc -i -
```

(Limitación: ffplay no usa el `pts`, así que el ritmo no es exacto. El cliente
`skry` —FFmpeg/SDL2— lo resuelve presentando por `pts`.)

## CI

`.github/workflows/ci.yml` corre, en cada push y PR:

- **Cliente Linux**: dentro de la imagen Docker (`build/client.Dockerfile`, con
  FFmpeg/SDL2) → paridad exacta con local. Formato + clippy + tests de todo el
  workspace (incluye `skry` y `skry-video`).
- **Cliente Windows**: build nativo de los crates sin deps nativas
  (`skry-proto`, `skry-adb`, `transport-spike`). El build de `skry`/`skry-video`
  con FFmpeg/SDL2 vía vcpkg es lo que falta (ADR-0007).
- **Server protocolo (Kotlin/JVM)**: `./gradlew :protocol:test` en la imagen de
  gradle pineada por digest.
