# ADR 0006: Mínimo privilegio y artefactos limpios

- Estado: Aceptada
- Fecha: 2026-06-16

## Contexto

Postura de seguridad transversal: ningún componente de skry debe correr con más
privilegios de los necesarios, y las imágenes que producen artefactos no deben
arrastrar el toolchain ni los fuentes de build.

## Decisión

### Contenedores sin root

- La imagen de build de dev/CI (`build/client.Dockerfile`) crea un usuario
  `builder` (uid 10001) y declara `USER builder`. Nunca corre como root.
- El wrapper `scripts/dev` además mapea el usuario del host (`--user uid:gid`)
  para que los artefactos del volumen montado no queden root-owned.
- La CI usa `scripts/dev` para todas las tareas de cargo: corre como el usuario
  del runner (no root), con paridad exacta al desarrollo local.
- La imagen de runtime (`build/release.Dockerfile`) crea un usuario `skry`
  (uid 10001) y declara `USER skry`.

### Artefactos limpios (multi-stage)

`build/release.Dockerfile` es multi-stage:

- **builder**: toolchain completo + fuentes → compila `--release --locked`.
- **runtime**: `debian-slim` con sólo las libs de runtime (sin compiladores, sin
  `-dev`, sin fuentes) + el binario. No se "contamina" con basura de build.
- **export**: `FROM scratch` con sólo el binario, para extraerlo con
  `docker build --target export --output`.

La imagen de dev/CI es un *entorno* de build (corre contra el código montado, no
hornea un artefacto), así que no necesita multi-stage; sí corre sin root.

### Mínimo privilegio en runtime real

- El **server Android** corre como uid `shell` (2000) vía `app_process`: no es
  root ni una app instalada con permisos amplios; sólo lo que `shell` permite
  (que alcanza para capturar pantalla vía las hidden APIs, ADR-0002).
- El **cliente** corre como el usuario normal del host. No requiere root: ADB,
  GPU y ventana son accesibles sin privilegios elevados.

## Consecuencias

- **Positivas**: superficie de ataque reducida; un fallo en el build no corre
  como root; las imágenes de release son chicas y sin herramientas de build; el
  runtime real es de mínimo privilegio de punta a punta.
- **Negativas**: correr la imagen de dev sin `--user` usa uid 10001, que puede
  no coincidir con el dueño de un volumen montado; por eso `scripts/dev` mapea
  el uid del host. Documentado.
