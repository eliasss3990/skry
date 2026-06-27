# skry

Espejá la pantalla de tu Android en la PC, con baja latencia, desde la terminal.

`skry` (de *scry*, "ver a distancia") es una herramienta de línea de comandos:
el teléfono captura y codifica su pantalla y la PC la recibe, decodifica y la
pinta en una ventana. Un solo comando, sin configuración, ejecutable desde
cualquier carpeta.

```bash
skry
```

> **Este README describe el diseño completo. Para lo que ya está construido vs.
> planeado, ver la tabla de estado en
> [docs/architecture.md](docs/architecture.md).** Lo marcado *(previsto)* abajo
> es el objetivo, todavía no implementado.

Con los defaults el sistema elige una configuración sensata según tu teléfono
(tasa nativa del panel acotada a 60 FPS, H.265 si hay encoder por hardware;
decode por GPU con fallback a CPU *(previsto)*). Para forzar parámetros
*(previsto — hoy el binario sólo expone `--serial` y `--fullscreen`)*:

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

## App Android (sin cable, sin adb) *(en construcción)*

Además del camino por `adb`, hay una **app Android nativa** (en `android-app/`)
que captura con `MediaProjection` —un toque para autorizar, sin depuración ni
cable— y sirve el mismo stream por la red local. La app se anuncia por mDNS
(`_skry._tcp`) y, mientras transmite, muestra en pantalla a qué dirección
conectar la PC.

Flujo:

1. En el teléfono: abrir skry, tocar **Iniciar captura** y aceptar el permiso.
   La pantalla muestra `IP:7345`.
2. En la PC: conectar directo a esa dirección, sin adb:

   ```bash
   skry --connect 192.168.1.50:7345
   ```

La app corre como servicio en primer plano (sobrevive a que pases a otra cosa en
el teléfono) y avisa —sin instalar nada solo— cuando hay una versión nueva.

> La **pantalla independiente** (`--new-display`) sigue por el camino `adb`: crear
> una pantalla virtual con permiso para lanzar apps requiere privilegios que una
> app normal no tiene. El descubrimiento mDNS desde el cliente (para no tipear la
> IP) queda como mejora pendiente; hoy la dirección se pasa a mano.

## Requisitos

- Un teléfono Android con **Depuración** activada (USB o inalámbrica). En
  Android 11+ la depuración inalámbrica se empareja por código, sin cable.
- Teléfono y PC en la **misma red Wi-Fi local** (el router no necesita internet).
- `adb` disponible (incluido en las Android Platform Tools).
- En la PC: drivers de la GPU al día para decode por hardware (opcional; hay
  fallback a CPU).

## Instalación

> **Todavía no disponible**: los scripts `install.sh`/`install.ps1` y los
> binarios se publican con el primer release. Los comandos de abajo son la forma
> de instalación prevista (un solo comando), aún no funcional.

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

## Cómo funciona (diseño)

1. El cliente verifica el dispositivo por ADB, despliega un `.jar` efímero a
   `/data/local/tmp/` y lo corre con `app_process` (no instala ninguna app).
   *Hoy el jar se empuja a mano; embeberlo y empujarlo desde el binario está
   previsto.*
2. Cliente y server negocian capacidades y parámetros (handshake).
3. El teléfono captura su pantalla, la codifica con MediaCodec y la envía.
4. La PC decodifica y renderiza. El ajuste de "marcha" según red/térmica via el
   canal de control está *previsto* (el protocolo ya lo soporta; falta cablearlo).

Al cerrar el cliente, el proceso del teléfono muere y no quedan rastros
persistentes.

> **Validado en device** (captura + encode + transporte end-to-end, S24 Ultra /
> Android 16): ver [docs/spikes.md](docs/spikes.md). El render propio (FFmpeg +
> SDL2) y el build de Windows son el trabajo en curso.

## Licencia

[MIT](LICENSE).
