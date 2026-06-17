# skry

Espejá la pantalla de tu Android en la PC, con baja latencia, desde la terminal.

`skry` (de *scry*, "ver a distancia") es una herramienta de línea de comandos:
el teléfono captura y codifica su pantalla y la PC la recibe, decodifica y la
pinta en una ventana. Un solo comando, sin configuración, ejecutable desde
cualquier carpeta.

```bash
skry
```

Con los defaults el sistema elige una configuración sensata según tu teléfono
(tasa nativa del panel acotada a 60 FPS, H.265 si hay encoder por hardware,
decode por GPU con fallback a CPU). Para forzar parámetros:

```bash
skry --gear 144 --codec h264 --hw-decode false --log-level debug
```

> **Alcance**: `skry` espeja **tu propio dispositivo**, autorizado por vos vía
> Depuración de Android. Requiere tu autorización explícita en la pantalla del
> propio teléfono. No es una herramienta de acceso a dispositivos ajenos.

**Inalámbrico y sin datos.** `skry` funciona por Wi-Fi (teléfono a unos metros
de la PC, sin cable) y es **100% local**: el video viaja sólo por tu red local,
nunca por internet, sin consumir datos móviles ni pasar por ningún servidor.

## Estado

En desarrollo activo. El cliente (Rust) y el server (Android/Kotlin) se
construyen sobre el túnel ADB; ver [docs/architecture.md](docs/architecture.md)
para el diseño y [docs/decisions/](docs/decisions/) para las decisiones de
arquitectura.

## Requisitos

- Un teléfono Android con **Depuración** activada (USB o inalámbrica). En
  Android 11+ la depuración inalámbrica se empareja por código, sin cable.
- Teléfono y PC en la **misma red Wi-Fi local** (el router no necesita internet).
- `adb` disponible (incluido en las Android Platform Tools).
- En la PC: drivers de la GPU al día para decode por hardware (opcional; hay
  fallback a CPU).

## Instalación

> Disponible al publicarse el primer release. Instalación de un comando:

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/eliasss3990/skry/main/scripts/install.ps1 | iex
```

**Linux / macOS (Bash):**

```bash
curl -sSL https://raw.githubusercontent.com/eliasss3990/skry/main/scripts/install.sh | bash
```

Ambos scripts dejan el binario en el PATH global para que `skry` se invoque
desde cualquier carpeta — la experiencia es un solo comando, como `gh`.

> Nota de distribución: `skry` enlaza FFmpeg y SDL2 (libs nativas). Según la
> plataforma, el paquete de instalación puede incluir esas libs junto al binario
> (p. ej. DLLs en Windows); el install script las coloca de modo que vos sólo
> escribís `skry`. Es decir: la *experiencia* es la de un binario único en el
> PATH, aunque el paquete no sea literalmente un solo archivo.

## Build desde el código

El toolchain de build está dockerizado para ser reproducible e idéntico a CI
(ver [ADR-0004](docs/decisions/0004-build-docker-runtime-host.md)). El runtime
corre nativo en tu host. Instrucciones en [docs/build.md](docs/build.md).

## Cómo funciona

1. El cliente verifica el dispositivo por ADB, empuja un `.jar` efímero a
   `/data/local/tmp/` y lo corre con `app_process` (no instala ninguna app).
2. Cliente y server negocian capacidades y parámetros (handshake).
3. El teléfono captura su pantalla, la codifica con MediaCodec y la envía.
4. La PC decodifica y renderiza, ajustando la "marcha" de fluidez según la
   estabilidad de la red y la térmica del teléfono.

Al cerrar el cliente, el proceso del teléfono muere y no quedan rastros
persistentes.

## Licencia

[MIT](LICENSE).
